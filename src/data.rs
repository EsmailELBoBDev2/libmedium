/*
 * Copyright (C) 2021  Aravinth Manivannan <realaravinth@batsense.net>
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as
 * published by the Free Software Foundation, either version 3 of the
 * License, or (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU Affero General Public License for more details.
 *
 * You should have received a copy of the GNU Affero General Public License
 * along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */
use std::path::Path;

use actix_web::web;
use graphql_client::{reqwest::post_graphql, GraphQLQuery};
use reqwest::header::USER_AGENT;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sled::{Db, Tree};

use crate::proxy::StringUtils;
use crate::SETTINGS;

const POST_CACHE_VERSION: usize = 1;
const GIST_CACHE_VERSION: usize = 1;

#[derive(Clone)]
pub struct Data {
    pub client: Client,
    cache: Db,
    pub posts: Tree,
    pub gists: Tree,
}

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "schemas/schema.graphql",
    query_path = "schemas/query.graphql",
    response_derives = "Debug, Serialize, Deserialize, Clone"
)]
pub struct GetPost;

pub type PostResp = get_post::GetPostPost;

pub type AppData = web::Data<Data>;

impl PostResp {
    pub fn get_gist_id<'a>(&self, url: &'a str) -> &'a str {
        url.split('/').last().unwrap()
    }
}

#[derive(Deserialize, Serialize)]
pub struct GistContent {
    pub files: Vec<GistFile>,
    pub html_url: String,
}

#[derive(Deserialize, Serialize)]
pub struct GistFile {
    pub file_name: String,
    pub content: String,
    pub language: String,
    pub raw_url: String,
}

impl GistFile {
    pub fn get_html_content(&self) -> String {
        let mut content = self.content.as_str();
        if self.content.starts_with('"') {
            content = self.content.slice(1..);
        }

        if content.ends_with('"') {
            content = content.slice(..content.len() - 1);
        }
        content.replace("\\t", "  ")
    }
}

impl Data {
    pub fn new() -> AppData {
        let path = Path::new(SETTINGS.cache.as_ref().unwrap()).join("posts_cache");
        let cache = sled::open(path).unwrap();
        let posts = cache.open_tree("posts").unwrap();
        let gists = cache.open_tree("gists").unwrap();
        let res = Self {
            client: Client::new(),
            cache,
            posts,
            gists,
        };
        res.migrate();

        AppData::new(res)
    }

    fn migrate(&self) {
        const POST_KEY: &str = "POST_CACHE_VERSION";
        const GIST_KEY: &str = "GIST_CACHE_VERSION";
        let trees = [
            (&self.posts, POST_KEY, POST_CACHE_VERSION),
            (&self.gists, GIST_KEY, GIST_CACHE_VERSION),
        ];

        for (tree, key, current_version) in trees {
            if let Ok(Some(v)) = tree.get(key) {
                let version = bincode::deserialize::<usize>(&v[..]).unwrap();
                let clean = !(version == current_version);

                if clean {
                    log::info!(
                        "Upgrading {} from version {} to version {}",
                        key,
                        version,
                        current_version
                    );
                    tree.clear().unwrap();
                    tree.flush().unwrap();
                    tree.insert(key, bincode::serialize(&current_version).unwrap())
                        .unwrap();
                }
            }
        }
    }

    pub async fn get_post(&self, id: &str) -> PostResp {
        match self.posts.get(id) {
            Ok(Some(v)) => bincode::deserialize(&v[..]).unwrap(),
            _ => {
                let vars = get_post::Variables { id: id.to_owned() };
                const URL: &str = "https://medium.com/_/graphql";

                let res = post_graphql::<GetPost, _>(&self.client, URL, vars)
                    .await
                    .unwrap();
                let res = res.data.expect("missing response data").post.unwrap();
                self.posts
                    .insert(id, bincode::serialize(&res).unwrap())
                    .unwrap();
                res
            }
        }
    }

    pub async fn get_gist(&self, id: String) -> (String, GistContent) {
        match self.gists.get(&id) {
            Ok(Some(v)) => (id, bincode::deserialize(&v[..]).unwrap()),
            _ => {
                const URL: &str = "https://api.github.com/gists/";

                let url = format!("{}{}", URL, id);

                let resp = self
                    .client
                    .get(&url)
                    .header(USER_AGENT, "libmedium")
                    .send()
                    .await
                    .unwrap()
                    .json::<serde_json::Value>()
                    .await
                    .unwrap();
                let files = resp.get("files").unwrap();
                let v = files.as_object().unwrap();
                let mut files = Vec::with_capacity(v.len());
                v.iter().for_each(|(name, file_obj)| {
                    let file = GistFile {
                        file_name: name.to_string(),
                        content: file_obj
                            .get("content")
                            .unwrap()
                            .as_str()
                            .unwrap()
                            .to_owned(),
                        language: file_obj
                            .get("language")
                            .unwrap()
                            .as_str()
                            .unwrap()
                            .to_owned(),
                        raw_url: file_obj
                            .get("raw_url")
                            .unwrap()
                            .as_str()
                            .unwrap()
                            .to_owned(),
                    };
                    files.push(file);
                });
                let gist = GistContent {
                    files,
                    html_url: resp.get("html_url").unwrap().to_string(),
                };

                self.gists
                    .insert(&id, bincode::serialize(&gist).unwrap())
                    .unwrap();
                (id, gist)
            }
        }
    }
}
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
use std::ops::{Bound, RangeBounds};

use actix_web::{web, HttpResponse, Responder};
use sailfish::TemplateOnce;

use crate::data::PostResp;
use crate::AppData;

pub mod routes {
    pub struct Proxy {
        pub index: &'static str,
        pub page: &'static str,
    }

    impl Proxy {
        pub const fn new() -> Self {
            Self {
                index: "/",
                page: "/{username}/{post}",
            }
        }
        pub fn get_page(&self, username: &str, post: &str) -> String {
            self.page
                .replace("{username}", username)
                .replace("{post}", post)
        }
    }
}

// credits @carlomilanesi:
// https://users.rust-lang.org/t/how-to-get-a-substring-of-a-string/1351/11
trait StringUtils {
    fn substring(&self, start: usize, len: usize) -> &str;
    fn slice(&self, range: impl RangeBounds<usize>) -> &str;
}

impl StringUtils for str {
    fn substring(&self, start: usize, len: usize) -> &str {
        let mut char_pos = 0;
        let mut byte_start = 0;
        let mut it = self.chars();
        loop {
            if char_pos == start {
                break;
            }
            if let Some(c) = it.next() {
                char_pos += 1;
                byte_start += c.len_utf8();
            } else {
                break;
            }
        }
        char_pos = 0;
        let mut byte_end = byte_start;
        loop {
            if char_pos == len {
                break;
            }
            if let Some(c) = it.next() {
                char_pos += 1;
                byte_end += c.len_utf8();
            } else {
                break;
            }
        }
        &self[byte_start..byte_end]
    }
    fn slice(&self, range: impl RangeBounds<usize>) -> &str {
        let start = match range.start_bound() {
            Bound::Included(bound) | Bound::Excluded(bound) => *bound,
            Bound::Unbounded => 0,
        };
        let len = match range.end_bound() {
            Bound::Included(bound) => *bound + 1,
            Bound::Excluded(bound) => *bound,
            Bound::Unbounded => self.len(),
        } - start;
        self.substring(start, len)
    }
}

#[derive(TemplateOnce)]
#[template(path = "post.html")]
#[template(rm_whitespace = true)]
pub struct Post {
    pub data: PostResp,
    pub id: String,
}

const INDEX: &str = include_str!("../templates/index.html");

#[my_codegen::get(path = "crate::V1_API_ROUTES.proxy.index")]
async fn index() -> impl Responder {
    HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .body(INDEX)
}

#[my_codegen::get(path = "crate::V1_API_ROUTES.proxy.page")]
async fn page(path: web::Path<(String, String)>, data: AppData) -> impl Responder {
    let post_id = path.1.split("-").last();
    if post_id.is_none() {
        return HttpResponse::BadRequest().finish();
    }
    let id = post_id.unwrap();

    let page = Post {
        id: id.to_owned(),
        data: data.get_post(&id).await,
    }
    .render_once()
    .unwrap();
    HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .body(page)
}

pub fn services(cfg: &mut web::ServiceConfig) {
    cfg.service(page);
    cfg.service(index);
}

#[cfg(test)]
mod tests {
    use actix_web::{http::StatusCode, test, App};

    use crate::{services, Data};

    #[actix_rt::test]
    async fn deploy_update_works() {
        let data = Data::new();
        let app = test::init_service(App::new().app_data(data.clone()).configure(services)).await;
        let urls = vec![
            "/@ftrain/big-data-small-effort-b62607a43a8c",
            "/geekculture/rest-api-best-practices-decouple-long-running-tasks-from-http-request-processing-9fab2921ace8",
            "/illumination/5-bugs-that-turned-into-features-e9a0e972a4e7",
            "/"
        ];

        for uri in urls.iter() {
            let resp =
                test::call_service(&app, test::TestRequest::get().uri(uri).to_request()).await;
            assert_eq!(resp.status(), StatusCode::OK);
        }
    }
}
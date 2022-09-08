use actix_web::{get, post, put, web, HttpRequest, HttpResponse, Responder};
use serde::{Deserialize, Serialize};

use crate::USER_LIST;

#[post("/users")]
async fn post_users(_user: web::Json<User>) -> impl Responder {
    let mut user = _user.clone();
    if user.id.is_none() {
        // # 添加用户

        // 是否重名
        let mut lock = USER_LIST.lock().unwrap();
        if lock.iter().any(|x| x.name == user.name) {
            return HttpResponse::BadRequest().json(Job {
                code: 1,
                reason: "ERR_INVALID_ARGUMENT".to_string(),
                message: format!("User name '{}' already exists.", user.name),
            });
        } else {
            // 不重名，更新其用户名
            user.id = Some(lock.len() as u32);
            lock.push(user.clone());
        }

        // ^ 添加用户
    } else {
        // # 重命名

        // id是否存在
        let mut lock = USER_LIST.lock().unwrap();
        if let Some(index) = lock.iter().position(|x| x.id == user.id) {
            // // id对应的用户存在，
            // // 判断新用户名是否与其他用户重名
            if lock.iter().any(|x| x.name == user.name.clone()) {
                return HttpResponse::BadRequest().json(Job {
                    code: 1,
                    reason: "ERR_INVALID_ARGUMENT".to_string(),
                    message: format!("User name '{}' already exists.", user.name),
                });
            } else {
                lock[index].name = user.name.clone();
            }
        } else {
            // id对应的用户不存在
            return HttpResponse::NotFound().json(Job {
                code: 3,
                reason: "ERR_NOT_FOUND".to_string(),
                message: format!("User {} not found.", _user.id.unwrap()),
            });
        };

        // ^ 重命名
    }
    // let mut lock = USER_LIST.lock().unwrap();
    // lock.push(user.clone());
    HttpResponse::Ok().json(user)
}

#[get("users")]
async fn get_users() -> impl Responder {
    let mut lock = USER_LIST.lock().unwrap();

    // to-do:  sort by user.id
    HttpResponse::Ok().json(&*lock)
}

#[derive(Debug, Deserialize, Clone, Serialize)]
pub struct User {
    pub id: Option<u32>,
    pub name: String,
}
#[derive(Serialize)]
struct Job {
    code: u32,
    reason: String,
    message: String,
}

use actix_web::{middleware::Logger, web, App, HttpServer};
use env_logger;
use lazy_static::lazy_static;
use log;

use serde_json;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

mod api;
mod args;
mod config;

use api::hello::{exit, greet};
use api::jobs::{get_jobs, get_jobs_by_id, post_jobs, put_jobs_by_id, JobCounter, JobResponse};
use args::Args;
use args::Parser;
use config::Config;

// 全局变量

lazy_static! {
    static ref JOB_LIST: Arc<Mutex<Vec<JobResponse>>> = Arc::new(Mutex::new(Vec::new()));
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // 初始化Logger
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));
    log::info!("starting HTTP server at http://localhost:12345");

    // 读取config
    let args = Args::parse();
    let config_path = Path::new(&args.config);
    let config_str = fs::read_to_string(config_path)?;
    let config: Config = serde_json::from_str(&config_str)?;

    // 创建job counter
    let counter = web::Data::new(JobCounter {
        counter: Mutex::new(-1),
    });

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(config.clone()))
            .app_data(counter.clone())
            .wrap(Logger::default())
            .route("/hello", web::get().to(|| async { "Hello World!" }))
            .service(greet)
            // DO NOT REMOVE: used in automatic testing
            .service(exit)
            .service(post_jobs)
            .service(get_jobs)
            .service(get_jobs_by_id)
            .service(put_jobs_by_id)
    })
    .bind(("127.0.0.1", 12345))?
    .run()
    .await
}

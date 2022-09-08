use std::fs;
use std::io::prelude::*;
use std::process::Stdio;
use std::time::{Duration, Instant};
use std::{fs::File, path::Path, process::Command, sync::Mutex};

use actix_web::{get, post, put, web, HttpRequest, HttpResponse, Responder};
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::{CONTEST_LIST, JOB_LIST, USER_LIST};
use chrono::Utc;
use wait_timeout::ChildExt;

#[derive(Debug, Deserialize, Clone, Serialize)]
pub struct PostJob {
    pub source_code: String,
    pub language: String,
    pub user_id: u32,
    pub contest_id: u32,
    pub problem_id: u32,
}

#[derive(Debug, Deserialize, Clone, Serialize)]
struct GetJob {
    language: Option<String>,
    user_id: Option<u32>,
    user_name: Option<String>,
    contest_id: Option<u32>,
    problem_id: Option<u32>,
    from: Option<String>,
    to: Option<String>,
    state: Option<State>,
    result: Option<JudgeResult>,
}

#[derive(Serialize)]
struct Job {
    code: u32,
    reason: String,
    message: String,
}

pub struct JobCounter {
    pub counter: Mutex<i32>,
}

/// body: 请求的正文, config: 配置信息
#[post("/jobs")]
async fn post_jobs(
    body: web::Json<PostJob>,
    config: web::Data<Config>,
    counter: web::Data<JobCounter>,
) -> impl Responder {
    // # 检查请求的合法性

    // 检查编程语言是否在配置中, 检查题目 ID 是否在配置中
    // 若不合法，返回错误响应
    let current_language = &body.language;
    let config_languages = &config.languages;
    let current_problem_id = &body.problem_id;
    let config_problems = &config.problems;

    if !config_languages.iter().any(|x| &x.name == current_language)
        || !config_problems.iter().any(|x| &x.id == current_problem_id)
    {
        return HttpResponse::NotFound().json(Job {
            code: 3,
            reason: "ERR_NOT_FOUND".to_string(),
            message: "HTTP 404 Not Found".to_string(),
        });
    }

    // 检查用户 ID 是否存在
    let lock = USER_LIST.lock().unwrap();
    if !lock.iter().any(|x| x.id.unwrap() == body.user_id) {
        return HttpResponse::NotFound().json(Job {
            code: 3,
            reason: "ERR_NOT_FOUND".to_string(),
            message: "HTTP 404 Not Found".to_string(),
        });
    }
    drop(lock);

    // # 比赛功能
    // 检查比赛 ID 是否存在
    let lock = CONTEST_LIST.lock().unwrap();
    let contest_index = lock
        .iter()
        .position(|x| x.id == Some(body.contest_id as usize));
    if contest_index.is_none() {
        return HttpResponse::NotFound().json(Job {
            code: 3,
            reason: "ERR_NOT_FOUND".to_string(),
            message: "HTTP 404 Not Found".to_string(),
        });
    }

    // 检查用户 ID 是否在此比赛中
    let contest = lock[contest_index.unwrap()].clone();
    if !contest.user_ids.contains(&(body.user_id as usize)) {
        return HttpResponse::BadRequest().json(Job {
            code: 1,
            reason: "ERR_INVALID_ARGUMENT".to_string(),
            message: "HTTP 400 Bad Request".to_string(),
        });
    }

    // 检查题目ID是否在此比赛中
    if !contest.problem_ids.contains(&(body.problem_id as usize)) {
        return HttpResponse::BadRequest().json(Job {
            code: 1,
            reason: "ERR_INVALID_ARGUMENT".to_string(),
            message: "HTTP 400 Bad Request".to_string(),
        });
    }
    drop(lock);

    // 用户该题目的提交次数限制是否达到上限
    // 在joblist中检索所有userid,problemid，contest_id和当前一样的提交
    let lock = JOB_LIST.lock().unwrap();
    let v: Vec<&JobResponse> = lock
        .iter()
        .filter(|x| x.submission.user_id == body.user_id)
        .filter(|x| x.submission.problem_id == body.problem_id)
        .filter(|x| x.submission.contest_id == body.contest_id)
        .collect();
    if (v.len() as i32) >= contest.submission_limit {
        return HttpResponse::BadRequest().json(Job {
            code: 4,
            reason: "ERR_RATE_LIMIT".to_string(),
            message: "HTTP 400 Bad Request".to_string(),
        });
    }
    drop(lock);

    // ^ 请求合法

    // # 实现阻塞评测（在评测结束时发送响应）

    // 维护测评id
    let mut cnt = counter.counter.lock().unwrap();
    *cnt += 1;
    let id = *cnt;

    // 维护测评time stamp
    let created_time = Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
    let updated_time = created_time.clone();

    // 维护当前测评的请求json
    let submission = body.to_owned();

    // 维护测评状态的变量
    let mut state = State::Queueing; // 整体测评状态
    let mut job_result = JudgeResult::Waiting; // 整体测评结果
    let mut compilation_result = JudgeResult::Waiting;
    let mut case_result = JudgeResult::Waiting; // 当前测试点测评结果

    // # 编译

    // 维护当前测评的语言配置 language
    let mut language_iter = config_languages.iter();
    let language = language_iter
        .find(|&x| &x.name == current_language)
        .unwrap();

    // 维护当前测评对应的题目 problem
    let mut problem_iter = config_problems.iter();
    let problem = problem_iter.find(|&x| &x.id == current_problem_id).unwrap();

    // 创建临时测评目录  e.g. TMPDIR/0/
    let temp_dir = Path::new("TMPDIR").join(id.to_string());
    fs::create_dir_all(&temp_dir).unwrap();

    // # 创建源代码文件  e.g. main.rs
    let src_file = temp_dir.join(&language.file_name);

    let display = src_file.display();
    let mut file = match File::create(&src_file) {
        Err(why) => panic!("couldn't create {}: {:?}", display, why),
        Ok(file) => file,
    };

    match file.write_all(submission.source_code.as_bytes()) {
        Err(why) => {
            panic!("couldn't write to {}: {:?}", display, why)
        }
        Ok(_) => println!("successfully wrote to {}", display),
    }
    // ^ 源码写入完毕

    // # 根据编程语言配置，将源代码编译成可执行文件 e.g. main

    // 更新测评状态
    state = State::Running;
    job_result = JudgeResult::Running;
    compilation_result = JudgeResult::Running;

    // get language-specific commands
    let commands = &language.command;
    let commands: Vec<String> = commands
        .iter()
        .map(|x| {
            if x == "%OUTPUT%" {
                temp_dir.join("main").to_str().unwrap().to_string()
            } else if x == "%INPUT%" {
                src_file.to_str().unwrap().to_string()
            } else {
                x.to_string()
            }
        })
        .collect();
    eprintln!("commands = {:?}", commands);

    // 子进程：编译
    let status = Command::new(&commands[0])
        .args(&commands[1..])
        .status()
        .unwrap();
    // ^ 编译完成

    // 错误处理：编译
    // 编译成功
    if status.code().unwrap() == 0 {
        compilation_result = JudgeResult::CompilationSuccess;
    } else {
        // 编译失败
        compilation_result = JudgeResult::CompilationError;
        job_result = JudgeResult::CompilationError;
        state = State::Finished;
    }

    // #按照顺序对数据点进行评测

    // 维护测试点信息
    let problem_cases = &problem.cases;

    // 维护测试结果相关信息
    let mut score = 0.0;
    let mut test_case_id = 0;
    let mut test_cases: Vec<Case> = Vec::new();
    let mut time = 0;
    let memory = 0;
    let info = "".to_string();

    // case 0: 编译
    test_cases.push(Case {
        id: 0,
        result: compilation_result,
        time: 0,
        memory,
        info: info.clone(),
    });

    // other cases
    for problem_case in problem_cases.iter() {
        test_case_id += 1;

        if job_result == JudgeResult::Running {
            // 指定in out文件
            let in_file = File::open(&problem_case.input_file).unwrap();
            let out_file = File::create(temp_dir.join("test.out")).unwrap();

            // 维护计时器s
            let now = Instant::now();

            let mut child = Command::new(temp_dir.join("main"))
                .stdin(Stdio::from(in_file))
                .stdout(Stdio::from(out_file))
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            let time_limit = problem_case.time_limit;
            let status_code = match child
                .wait_timeout(Duration::from_micros(time_limit))
                .unwrap()
            {
                Some(status) => status.code(),
                None => {
                    // child hasn't exited yet
                    child.kill().unwrap();
                    child.wait().unwrap().code()
                }
            };

            time = now.elapsed().as_micros() as u64;

            // 错误处理: 程序运行返回值
            if status_code == Some(101) {
                case_result = JudgeResult::RuntimeError;
                job_result = case_result;
            } else if status_code == Some(0) {
                // # 运行成功，比较test.out & file.ans

                let out_str = fs::read_to_string(temp_dir.join("test.out")).unwrap();
                let ans_str = fs::read_to_string(&problem_case.answer_file).unwrap();
                eprintln!("out_str = {:?}", out_str);
                eprintln!("ans_str = {:?}", ans_str);

                if problem.typ == "standard" {
                    let out_str_vec: Vec<String> = out_str
                        .trim()
                        .split("\n")
                        .map(|s| s.trim().to_string())
                        .collect();
                    let ans_str_vec: Vec<String> = ans_str
                        .trim()
                        .split("\n")
                        .map(|s| s.trim().to_string())
                        .collect();
                    eprintln!("out_str_vec = {:?}", out_str_vec);
                    eprintln!("ans_str_vec = {:?}", ans_str_vec);
                    if out_str_vec == ans_str_vec {
                        score += &problem_case.score;
                        case_result = JudgeResult::Accepted;
                    } else {
                        case_result = JudgeResult::WrongAnswer;
                        job_result = JudgeResult::WrongAnswer;
                    }
                } else if problem.typ == "strict" {
                    if out_str == ans_str {
                        score += &problem_case.score;
                        case_result = JudgeResult::Accepted;
                    } else {
                        case_result = JudgeResult::WrongAnswer;
                        job_result = JudgeResult::WrongAnswer;
                    }
                }
            } else if status_code == None {
                // tle
                case_result = JudgeResult::TimeLimitExceeded;
                job_result = JudgeResult::TimeLimitExceeded;
            } else {
            }
        } else {
            case_result = JudgeResult::Waiting;
        }

        // 更新test_cases
        test_cases.push(Case {
            id: test_case_id,
            result: case_result,
            time,
            memory,
            info: info.clone(),
        })
    }
    // ^ 所有数据点测评完毕
    state = State::Finished;
    let mut it = test_cases.iter();
    it.next();
    if !it.any(|x| x.result != JudgeResult::Accepted) {
        job_result = JudgeResult::Accepted;
    }

    // 清理文件夹
    fs::remove_dir_all(&temp_dir).unwrap();

    // 返回正确响应
    let res = JobResponse {
        id,
        created_time,
        updated_time,
        submission,
        state,
        result: job_result,
        score,
        cases: test_cases,
    };
    let mut lock = JOB_LIST.lock().unwrap();
    lock.push(res.clone());
    HttpResponse::Ok().json(res)
}

#[get("/jobs")]
async fn get_jobs(req: HttpRequest) -> impl Responder {
    let info = match web::Query::<GetJob>::from_query(req.query_string()) {
        Err(_) => {
            return HttpResponse::BadRequest().json(Job {
                code: 1,
                reason: "ERR_INVALID_ARGUMENT".to_string(),
                message: "Invalid argument".to_string(),
            });
        }
        Ok(info) => info,
    };

    let lock = JOB_LIST.lock().unwrap();
    let mut slice = lock.clone();
    // filter
    if info.language.is_some() {
        slice = slice
            .into_iter()
            .filter(|x| &x.submission.language == info.language.as_ref().unwrap())
            .collect();
    }
    if info.result.is_some() {
        slice = slice
            .into_iter()
            .filter(|x| &x.result == info.result.as_ref().unwrap())
            .collect();
    }
    if info.problem_id.is_some() {
        slice = slice
            .into_iter()
            .filter(|x| &x.submission.problem_id == info.problem_id.as_ref().unwrap())
            .collect();
    }
    if info.state.is_some() {
        slice = slice
            .into_iter()
            .filter(|x| &x.state == info.state.as_ref().unwrap())
            .collect();
    }

    HttpResponse::Ok().json(slice)
}

#[get("/jobs/{jobid}")]
async fn get_jobs_by_id(jobid: web::Path<String>) -> impl Responder {
    eprintln!("jobid = {:?}", jobid);
    let id = match jobid.to_string().parse::<i32>() {
        Err(_) => {
            return HttpResponse::BadRequest().json(Job {
                reason: "ERR_NOT_FOUND".to_string(),
                code: 3,
                message: format!("job {} not found.", jobid),
            })
        }
        Ok(id) => id,
    };
    eprintln!("id = {:?}", id);
    let lock = JOB_LIST.lock().unwrap();
    let mut slice = lock.clone();
    slice = slice.into_iter().filter(|x| x.id == id).collect();
    HttpResponse::Ok().json(&slice[0])
}

#[put("jobs/{jobid}")]
async fn put_jobs_by_id(jobid: web::Path<String>, config: web::Data<Config>) -> impl Responder {
    eprintln!("jobid = {:?}", jobid);
    let id = match jobid.to_string().parse::<i32>() {
        Err(_) => {
            return HttpResponse::BadRequest().json(Job {
                reason: "ERR_NOT_FOUND".to_string(),
                code: 3,
                message: format!("job {} not found.", jobid),
            })
        }
        Ok(id) => id,
    };
    eprintln!("id = {:?}", id);
    let mut lock = JOB_LIST.lock().unwrap();
    let index = match lock.iter().position(|x| x.id == id) {
        Some(index) => index,
        None => {
            return HttpResponse::NotFound().json(Job {
                reason: "ERR_NOT_FOUND".to_string(),
                code: 3,
                message: format!("job {} not found.", jobid),
            })
        }
    };
    lock[index].updated_time = Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();

    let current_language = &lock[index].submission.language;
    let config_languages = &config.languages;
    let current_problem_id = &lock[index].submission.problem_id;
    let config_problems = &config.problems;
    if !config_languages.iter().any(|x| &x.name == current_language)
        || !config_problems.iter().any(|x| &x.id == current_problem_id)
    {
        return HttpResponse::NotFound().json(Job {
            code: 3,
            reason: "ERR_NOT_FOUND".to_string(),
            message: "HTTP 404 Not Found".to_string(),
        });
    }

    // 维护测评状态的变量
    let mut state = State::Queueing; // 整体测评状态
    let mut job_result = JudgeResult::Waiting; // 整体测评结果
    let mut compilation_result = JudgeResult::Waiting;
    let mut case_result = JudgeResult::Waiting; // 当前测试点测评结果

    let mut language_iter = config_languages.iter();
    let language = language_iter
        .find(|&x| &x.name == current_language)
        .unwrap();

    // 维护当前测评对应的题目 problem
    let mut problem_iter = config_problems.iter();
    let problem = problem_iter.find(|&x| &x.id == current_problem_id).unwrap();

    // 创建临时测评目录  e.g. TMPDIR/0/
    let temp_dir = Path::new("TMPDIR").join(id.to_string());
    fs::create_dir_all(&temp_dir).unwrap();

    // # 创建源代码文件  e.g. main.rs
    let src_file = temp_dir.join(&language.file_name);

    let display = src_file.display();
    let mut file = match File::create(&src_file) {
        Err(why) => panic!("couldn't create {}: {:?}", display, why),
        Ok(file) => file,
    };

    match file.write_all(&lock[index].submission.source_code.as_bytes()) {
        Err(why) => {
            panic!("couldn't write to {}: {:?}", display, why)
        }
        Ok(_) => println!("successfully wrote to {}", display),
    }
    // ^ 源码写入完毕

    // # 根据编程语言配置，将源代码编译成可执行文件 e.g. main

    // 更新测评状态
    state = State::Running;
    job_result = JudgeResult::Running;
    compilation_result = JudgeResult::Running;

    // get language-specific commands
    let commands = &language.command;
    let commands: Vec<String> = commands
        .iter()
        .map(|x| {
            if x == "%OUTPUT%" {
                temp_dir.join("main").to_str().unwrap().to_string()
            } else if x == "%INPUT%" {
                src_file.to_str().unwrap().to_string()
            } else {
                x.to_string()
            }
        })
        .collect();
    eprintln!("commands = {:?}", commands);

    // 子进程：编译
    let status = Command::new(&commands[0])
        .args(&commands[1..])
        .status()
        .unwrap();
    // ^ 编译完成

    // 错误处理：编译
    // 编译成功
    if status.code().unwrap() == 0 {
        compilation_result = JudgeResult::CompilationSuccess;
    } else {
        // 编译失败
        compilation_result = JudgeResult::CompilationError;
        job_result = JudgeResult::CompilationError;
        state = State::Finished;
    }

    // #按照顺序对数据点进行评测

    // 维护测试点信息
    let problem_cases = &problem.cases;

    // 维护测试结果相关信息
    let mut score = 0.0;
    let mut test_case_id = 0;
    let mut test_cases: Vec<Case> = Vec::new();
    let mut time = 0;
    let memory = 0;
    let info = "".to_string();

    // case 0: 编译
    test_cases.push(Case {
        id: 0,
        result: compilation_result,
        time: 0,
        memory,
        info: info.clone(),
    });

    // other cases
    for problem_case in problem_cases.iter() {
        test_case_id += 1;

        if job_result == JudgeResult::Running {
            // 指定in out文件
            let in_file = File::open(&problem_case.input_file).unwrap();
            let out_file = File::create(temp_dir.join("test.out")).unwrap();

            // 维护计时器s
            let now = Instant::now();

            let mut child = Command::new(temp_dir.join("main"))
                .stdin(Stdio::from(in_file))
                .stdout(Stdio::from(out_file))
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            let time_limit = problem_case.time_limit;
            let status_code = match child
                .wait_timeout(Duration::from_micros(time_limit))
                .unwrap()
            {
                Some(status) => status.code(),
                None => {
                    // child hasn't exited yet
                    child.kill().unwrap();
                    child.wait().unwrap().code()
                }
            };

            time = now.elapsed().as_micros() as u64;

            // 错误处理: 程序运行返回值
            if status_code == Some(101) {
                case_result = JudgeResult::RuntimeError;
                job_result = case_result;
            } else if status_code == Some(0) {
                // # 运行成功，比较test.out & file.ans

                let out_str = fs::read_to_string(temp_dir.join("test.out")).unwrap();
                let ans_str = fs::read_to_string(&problem_case.answer_file).unwrap();
                eprintln!("out_str = {:?}", out_str);
                eprintln!("ans_str = {:?}", ans_str);

                if problem.typ == "standard" {
                    let out_str_vec: Vec<String> = out_str
                        .trim()
                        .split("\n")
                        .map(|s| s.trim().to_string())
                        .collect();
                    let ans_str_vec: Vec<String> = ans_str
                        .trim()
                        .split("\n")
                        .map(|s| s.trim().to_string())
                        .collect();
                    eprintln!("out_str_vec = {:?}", out_str_vec);
                    eprintln!("ans_str_vec = {:?}", ans_str_vec);
                    if out_str_vec == ans_str_vec {
                        score += &problem_case.score;
                        case_result = JudgeResult::Accepted;
                    } else {
                        case_result = JudgeResult::WrongAnswer;
                        job_result = JudgeResult::WrongAnswer;
                    }
                } else if problem.typ == "strict" {
                    if out_str == ans_str {
                        score += &problem_case.score;
                        case_result = JudgeResult::Accepted;
                    } else {
                        case_result = JudgeResult::WrongAnswer;
                        job_result = JudgeResult::WrongAnswer;
                    }
                }
            } else if status_code == None {
                // tle
                case_result = JudgeResult::TimeLimitExceeded;
                job_result = JudgeResult::TimeLimitExceeded;
            } else {
            }
        } else {
            case_result = JudgeResult::Waiting;
        }

        // 更新test_cases
        test_cases.push(Case {
            id: test_case_id,
            result: case_result,
            time,
            memory,
            info: info.clone(),
        })
    }
    // ^ 所有数据点测评完毕
    state = State::Finished;
    let mut it = test_cases.iter();
    it.next();
    if !it.any(|x| x.result != JudgeResult::Accepted) {
        job_result = JudgeResult::Accepted;
    }

    // 清理文件夹
    fs::remove_dir_all(&temp_dir).unwrap();

    lock[index].cases = test_cases;
    lock[index].score = score;
    lock[index].result = job_result;
    lock[index].state = state;
    //
    HttpResponse::Ok().json(&lock[index])
}

#[derive(Serialize, Clone)]
pub struct JobResponse {
    pub id: i32,
    pub created_time: String,
    pub updated_time: String,
    pub submission: PostJob,
    pub state: State,
    pub result: JudgeResult,
    pub score: f64,
    pub cases: Vec<Case>,
}

#[derive(Debug, Serialize, Clone, Deserialize, PartialEq)]
pub enum State {
    Queueing,
    Running,
    Finished,
    // Canceled,
}
#[derive(Serialize, PartialEq, Clone, Copy, Deserialize, Debug)]
pub enum JudgeResult {
    Waiting,
    Running,
    Accepted,
    #[serde(rename = "Compilation Error")]
    CompilationError,
    #[serde(rename = "Compilation Success")]
    CompilationSuccess,
    #[serde(rename = "Wrong Answer")]
    WrongAnswer,
    #[serde(rename = "Runtime Error")]
    RuntimeError,
    #[serde(rename = "Time Limit Exceeded")]
    TimeLimitExceeded,
    // #[serde(rename = "Memory Limit Exceeded")]
    // MemoryLimitExceeded,
    // #[serde(rename = "System Error")]
    // SystemError,
    // #[serde(rename = "SPJ Error")]
    // SPJError,
    // Skipped,
}

#[derive(Serialize, Clone)]
pub struct Case {
    id: i32,
    result: JudgeResult,
    time: u64,
    memory: u64,
    info: String,
}

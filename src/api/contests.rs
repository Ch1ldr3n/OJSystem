use actix_web::{get, post, web, HttpRequest, HttpResponse, Responder};
use serde::{Deserialize, Serialize};

use crate::{api::jobs::JobResponse, config::Config, CONTEST_LIST, JOB_LIST, USER_LIST};

use super::users::User;

#[get("/contests/{contestid}/ranklist")]
async fn get_ranklist(
    config: web::Data<Config>,
    req: HttpRequest,
    contestid: web::Path<usize>,
) -> impl Responder {
    println!("1");
    let info = match web::Query::<RankInfo>::from_query(req.query_string()) {
        Err(_) => {
            return HttpResponse::BadRequest().json(Job {
                code: 1,
                reason: "ERR_INVALID_ARGUMENT".to_string(),
                message: "Invalid argument".to_string(),
            });
        }
        Ok(info) => info,
    };

    println!("2");
    // 比赛信息
    let contest_id = contestid.into_inner();
    let clock = CONTEST_LIST.lock().unwrap();
    let cindex = clock.iter().position(|x| x.id == Some(contest_id));
    if cindex.is_none() {
        return HttpResponse::NotFound().json(Job {
            code: 3,
            reason: "ERR_NOT_FOUND".to_string(),
            message: "Contest {contest_id} not found.".to_string(),
        });
    }
    let cindex = cindex.unwrap();
    let contest = clock[cindex].clone();
    drop(clock);

    println!("3");
    // 比赛 id 为 0 总是表示全局排行榜，即包括所有的用户和所有的题目（按题目 id 升序）
    // id不为0时，根据比赛计算
    let problems_num = if contest_id == 0 {
        config.problems.len()
    } else {
        contest.problem_ids.len()
    };

    let closure = |x: usize| contest.problem_ids.iter().position(|a| a == &x).unwrap();

    println!("4");

    // user_id -> user submission cnt
    let mut submission_count: Vec<i32> = Vec::new();
    // user_id -> last scored submisssion time
    let mut submission_time: Vec<String> = Vec::new();

    // 用user list初始化rank list
    let mut rank_list: Vec<Rank> = Vec::new();
    if contest_id == 0 {
        let lock2 = USER_LIST.lock().unwrap();
        for user in lock2.iter() {
            rank_list.push(Rank::new(user.clone(), problems_num));
            submission_count.push(0);
            submission_time.push("zzz".to_string())
        }
        drop(lock2);
    } else {
        let lock2 = USER_LIST.lock().unwrap();
        for user_id in contest.user_ids.iter() {
            // user_id -> user , assert userlist[0,1,...]
            rank_list.push(Rank::new(lock2[*user_id].clone(), problems_num));
            submission_count.push(0);
            submission_time.push("zzz".to_string())
        }
        drop(lock2);
    }

    println!("5");

    // 如果id为0，遍历所有测评, 提取 user, problem id, score, submission_count
    // id 不为0， 只遍历当前contest的测评
    let lock = JOB_LIST.lock().unwrap();

    let mut vec = if contest_id == 0 {
        lock.clone()
    } else {
        lock.clone()
            .into_iter()
            .filter(|x| x.submission.contest_id as usize == contest_id)
            .collect()
    };

    println!("6");

    // 按problem id 排序
    vec.sort_by(|a, b| a.submission.problem_id.cmp(&b.submission.problem_id));

    for item in vec.iter() {
        let user_id = item.submission.user_id;
        submission_count[user_id as usize] += 1;
        let lock2 = USER_LIST.lock().unwrap();
        let user = lock2[lock2.iter().position(|x| x.id == Some(user_id)).unwrap()].clone();
        drop(lock2);
        // warning: 假设题目号都是从0开始递增
        let problem_id = closure(item.submission.problem_id as usize);
        let score = item.score;
        // 更新ranklist里该用户对应题目的分数
        for rank in rank_list.iter_mut() {
            if rank.user == user {
                if info.scoring_rule.is_some() && info.scoring_rule.clone().unwrap() == "highest" {
                    if score > rank.scores[problem_id] {
                        rank.scores[problem_id] = score;
                        // 绑定提交时间
                        submission_time[user_id as usize] = item.created_time.clone();
                    }
                } else {
                    rank.scores[problem_id] = score;
                    submission_time[user_id as usize] = item.created_time.clone();
                }
            }
        }
    }
    drop(lock);
    println!("7");

    // rank list scores and users updated
    // todo: sort by scores
    rank_list.sort_by(|a, b| {
        b.scores
            .iter()
            .sum::<f64>()
            .partial_cmp(&a.scores.iter().sum::<f64>())
            .unwrap()
    });
    println!("8");

    // compute rank
    if info.tie_breaker.is_none() {
        let snapshot = rank_list.clone();
        // update rank, if tie , rank same
        let mut currank = 0;
        for (index, item) in rank_list.iter_mut().enumerate() {
            if index == 0 {
                item.rank = 1;
                currank = 1;
            } else {
                if item.scores.iter().sum::<f64>() == snapshot[index - 1].scores.iter().sum::<f64>()
                {
                    item.rank = currank;
                } else {
                    item.rank = index as i32 + 1;
                    currank = item.rank;
                }
            }
        }
    } else {
        if info.tie_breaker.clone().unwrap() == "user_id" {
            for (index, item) in rank_list.iter_mut().enumerate() {
                item.rank = index as i32 + 1;
            }
        } else if info.tie_breaker.clone().unwrap() == "submission_count" {
            rank_list.sort_by(|a, b| {
                submission_count[a.user.id.unwrap() as usize]
                    .cmp(&submission_count[b.user.id.unwrap() as usize])
            });
            let mut cur_count = 0;
            let mut cur_rank = 0;
            for (index, item) in rank_list.iter_mut().enumerate() {
                if index == 0 {
                    item.rank = 1;
                    cur_rank = 1;
                    cur_count = submission_count[item.user.id.unwrap() as usize];
                } else {
                    if submission_count[item.user.id.unwrap() as usize] == cur_count {
                        item.rank = cur_rank;
                    } else {
                        item.rank = index as i32 + 1;
                        cur_rank = item.rank;
                    }
                }
            }
        } else if info.tie_breaker.clone().unwrap() == "submission_time" {
            rank_list.sort_by(|a, b| {
                submission_time[a.user.id.unwrap() as usize]
                    .cmp(&submission_time[b.user.id.unwrap() as usize])
            });
            for (index, item) in rank_list.iter_mut().enumerate() {
                item.rank = index as i32 + 1;
            }
        }
    }

    println!("8");

    HttpResponse::Ok().json(rank_list)

    // "s"
}

#[post("/contests")]
async fn post_contests(mut contest: web::Json<Contest>) -> impl Responder {
    // 检查id 字段是否存在
    if contest.id.is_none() {
        // id 字段不存在，新建比赛并返回比赛信息作为响应
        // 新建的比赛保证其 id 不与现有比赛重复
        let mut lock = CONTEST_LIST.lock().unwrap();
        contest.id = Some(lock.len() + 1);
        lock.push(contest.clone());
    }

    HttpResponse::Ok().json(contest)
}

#[get("/contests")]
async fn get_contests() -> impl Responder {
    let lock = CONTEST_LIST.lock().unwrap();
    HttpResponse::Ok().json(&*lock)
}

#[get("/contests/{contestid}")]
async fn get_contests_by_id(contestid: web::Path<String>) -> impl Responder {
    let id = match contestid.to_string().parse::<usize>() {
        Err(_) => {
            return HttpResponse::BadRequest().json(Job {
                reason: "ERR_NOT_FOUND".to_string(),
                code: 3,
                message: format!("Contest {} not found.", contestid),
            })
        }
        Ok(id) => id,
    };

    let lock = CONTEST_LIST.lock().unwrap();
    let index = lock.iter().position(|x| x.id.unwrap() == id);
    if index.is_none() {
        // 找不到比赛
        return HttpResponse::NotFound().json(Job {
            reason: "ERR_NOT_FOUND".to_string(),
            code: 3,
            message: format!("Contest {} not found.", contestid),
        });
    } else {
        let index = index.unwrap();
        return HttpResponse::Ok().json(&lock[index]);
    }
}

#[derive(Debug, Deserialize, Clone, Serialize, PartialEq)]
pub struct Contest {
    pub id: Option<usize>,
    pub name: String,
    pub from: String,
    pub to: String,
    pub problem_ids: Vec<usize>,
    pub user_ids: Vec<usize>,
    pub submission_limit: i32,
}

#[derive(Debug, Deserialize, Clone, Serialize)]
pub struct Rank {
    user: User,
    rank: i32,
    scores: Vec<f64>,
}

impl Rank {
    fn new(user: User, num: usize) -> Self {
        Rank {
            user,
            rank: 1,
            scores: vec![0.0; num],
        }
    }
}
#[derive(Debug, Deserialize, Clone, Serialize)]
struct RankInfo {
    scoring_rule: Option<String>,
    tie_breaker: Option<String>,
}
#[derive(Serialize)]
struct Job {
    code: u32,
    reason: String,
    message: String,
}

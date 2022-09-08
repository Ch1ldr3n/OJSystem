use actix_web::{get, post, web, HttpRequest, HttpResponse, Responder};
use serde::{Deserialize, Serialize};

use crate::{config::Config, JOB_LIST, USER_LIST};

use super::users::User;

#[get("/contests/{contestId}/ranklist")]
async fn get_ranklist(config: web::Data<Config>, req: HttpRequest) -> impl Responder {
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

    //比赛 id 为 0 总是表示全局排行榜，即包括所有的用户和所有的题目（按题目 id 升序）
    let problems_num = config.problems.len();

    // user_id -> user submission cnt
    let mut submission_count: Vec<i32> = Vec::new();
    // user_id -> last scored submisssion time
    let mut submission_time: Vec<String> = Vec::new();

    // 用user list初始化rank list
    let mut rank_list: Vec<Rank> = Vec::new();
    let lock2 = USER_LIST.lock().unwrap();
    for user in lock2.iter() {
        rank_list.push(Rank::new(user.clone(), problems_num));
        submission_count.push(0);
        submission_time.push("zzz".to_string())
    }
    drop(lock2);

    // 遍历所有测评, 提取 user, problem id, score, submission_count
    let lock = JOB_LIST.lock().unwrap();
    for item in lock.iter() {
        let user_id = item.submission.user_id;
        submission_count[user_id as usize] += 1;
        let lock2 = USER_LIST.lock().unwrap();
        let user = lock2[lock2.iter().position(|x| x.id == Some(user_id)).unwrap()].clone();
        drop(lock2);
        // warning: 假设题目号都是从0开始递增
        let problem_id = item.submission.problem_id as usize;
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

    // rank list scores and users updated
    // todo: sort by scores
    rank_list.sort_by(|a, b| {
        b.scores
            .iter()
            .sum::<f64>()
            .partial_cmp(&a.scores.iter().sum::<f64>())
            .unwrap()
    });

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

    HttpResponse::Ok().json(rank_list)

    // "s"
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

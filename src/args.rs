pub use clap::Parser;

/// 处理命令行参数的结构体
#[derive(Parser, Debug)]
pub struct Args {
    /// 指定配置文件
    #[clap(short, long, value_parser)]
    pub config: String,

    #[clap(short, long, action)]
    pub flush_data: bool,
}

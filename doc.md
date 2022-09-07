# POST /jobs

- 接受到 `POST /jobs` 请求时，从请求中获取 `source_code` 字段，根据配置文件中 Rust 语言的配置，使用 `rustc` 命令编译为可执行文件；运行 `rustc` 可以用 `std::process::Command`。

1. 创建api，

2. 从请求中获取字段



##### http响应：错误

```json
{
  "code": 3,
  "reason": "ERR_NOT_FOUND",
  "message": "Problem 123456 not found."
}
```


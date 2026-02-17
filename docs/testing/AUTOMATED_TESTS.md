# 自动化测试说明（Automated Tests）

## 目标

快速验证代码质量与基础功能回归，作为手动联测前的门禁。

## 推荐执行顺序

1. `cargo fmt --all`
2. `cargo clippy --all-targets --all-features -- -D warnings`
3. `cargo test`

## 命令清单

```bash
# 格式检查
cargo fmt --all

# 严格 lint
cargo clippy --all-targets --all-features -- -D warnings

# 单元测试 + 文档测试
cargo test
```

## CI 建议门禁

- `fmt` 无改动
- `clippy` 0 warning（按 `-D warnings`）
- `test` 全绿

## 当前自动化覆盖边界

自动化测试适合验证：

- 参数解析与帮助文本
- 配置默认值与错误映射
- 文本处理逻辑（如 monitor UTF-8/时间戳函数）

自动化测试不等价于真实设备联测，以下仍需手动验证：

- 串口稳定性与硬件时序
- 真实烧录成功率
- 中断后的资源回收（端口释放）
- 烧录后 monitor 联动体验

## CLI 兼容性矩阵

CLI 行为固化（flags / exit code / stdout-stderr / JSON / i18n）与测试映射详见：

- `docs/testing/CLI_COMPATIBILITY_MATRIX.md`

## 失败排查建议

- 如果 `clippy` 失败：优先修复 warning，再跑全套。
- 如果 `test` 失败：先最小化复现（单测名），再扩到全量。
- 如果 CLI 行为异常但自动化全绿：转到手动清单中的硬件联测项排查。

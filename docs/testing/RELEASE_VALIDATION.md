# 发布前验证（Release Validation）

> 适用于发布 `hisiflash-cli` 与 `hisiflash` crate 前的最终检查。

## 1) 版本与变更记录

- [ ] 版本号已按计划更新
- [ ] 对应 changelog 已更新（workspace 索引 + crate 级 changelog）
- [ ] 变更描述与实际行为一致（尤其退出码/交互契约）

## 2) 自动化门禁

- [ ] `cargo fmt --all` 通过
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` 通过
- [ ] `cargo test` 通过

## 3) 手动联测门禁

- [ ] 已完成 [MANUAL_CHECKLIST.md](MANUAL_CHECKLIST.md)
- [ ] 完整烧录成功路径验证通过（真实设备）
- [ ] 烧录后 monitor 联测通过（含中断语义）

## 4) 发布产物检查

- [ ] CLI 可执行文件可运行（`--help`、`list-ports`）
- [ ] 平台目标产物命名与下载链接正确
- [ ] 文档中的安装命令与版本号一致

## 5) 回滚与风险准备

- [ ] 已记录高风险改动点
- [ ] 如需回滚，具备明确回退提交/步骤
- [ ] 已准备发布后快速验证命令（smoke test）

## 发布结论

- 结论：允许发布 / 暂缓发布
- 负责人：
- 日期：
- 备注：

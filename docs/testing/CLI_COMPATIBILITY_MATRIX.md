# CLI 兼容性矩阵（Contract Matrix）

本文档定义 `hisiflash-cli` 当前“已固化”的 CLI 行为边界，并将每项契约映射到现有自动化测试，作为回归基线。

> 适用范围：`hisiflash-cli` 命令行行为（参数/退出码/输出流/JSON/i18n）。
>
> 不适用范围：真实硬件时序、串口物理链路稳定性、烧录成功率等硬件集成行为。

## 分级说明

- **P0（强契约）**：破坏即视为兼容性回归，必须阻止合入。
- **P1（稳态契约）**：默认保持兼容；确需变更应同步测试与文档并在变更说明中标注。
- **P2（实现约束）**：偏实现细节，不作为稳定外部契约。

---

## Flags / 命令语法

| 级别 | 契约 | 当前行为 | 测试映射 |
|---|---|---|---|
| P0 | 子命令集合稳定 | `flash/write/write-program/erase/info/list-ports/monitor/completions` 可解析 | `cli_tests::test_build_localized_command_has_subcommands` |
| P0 | 全局参数解析稳定 | `--port --baud --chip --lang -v -q --non-interactive --confirm-port --list-all-ports --config` | `cli_tests::test_cli_global_options` |
| P0 | 关键子命令参数语义稳定 | 各命令关键参数与默认值固定（如 `monitor --monitor-baud` 默认 `115200`） | `test_cli_parse_flash* / test_cli_parse_write* / test_cli_parse_info* / test_cli_parse_list_ports* / test_cli_parse_monitor*` |
| P0 | 十六进制地址解析规则稳定 | 支持 `0x`、下划线、大小写，非法输入报错 | `test_parse_hex_u32_*`、`test_parse_bin_arg_*` |
| P1 | 选项终止符 `--` 语义稳定 | `--` 后支持以 `-` 开头的操作数 | `option_terminator_allows_dash_prefixed_operand`、`option_terminator_with_flash_command` |

---

## Exit Codes

| 级别 | 契约 | 当前行为 | 测试映射 |
|---|---|---|---|
| P0 | 成功返回 0 | `--help`/`--version`/`completions bash` 返回 `0` | `exit_code_zero_on_success` |
| P0 | 用法错误返回 2 | 未知命令、非法 flag、参数缺失归类为 usage | `exit_code_two_for_usage_error_*` |
| P0 | 设备未找到返回 4 | 显式无效 `--port` 强制映射 `DeviceNotFound => 4` | `exit_code_four_for_device_not_found` |
| P0 | 取消语义保留 130 | `Cancelled` 类错误映射 `130` | `cli_tests::test_map_exit_code_cancelled_is_130` |
| P1 | 配置类错误语义 | `CliError::Config => 3`；但“损坏 TOML”当前为告警继续执行（非致命） | `exit_code_three_for_config_error_invalid_file` |
| P1 | 兜底错误返回 1 | 未分类异常映射 `1` | `exit_code_one_for_unexpected_error` |

---

## stdout / stderr 约定

| 级别 | 契约 | 当前行为 | 测试映射 |
|---|---|---|---|
| P0 | 帮助与版本走 stdout | `--help/-h/--version/-V` 成功且 `stderr` 为空 | `help_exits_zero_and_writes_stdout_only`、`short_help_exits_zero_and_writes_stdout_only`、`version_exits_zero_and_writes_stdout_only`、`short_version_exits_zero_and_writes_stdout_only` |
| P0 | 解析/参数错误走 stderr | 非 JSON 模式下错误信息不污染 stdout | `flash_command_writes_to_stderr_only`、`write_command_invalid_args_writes_to_stderr_only`、`erase_command_invalid_args_writes_to_stderr_only` |
| P1 | completions 输出到 stdout | `completions bash` 产物在 stdout，stderr 为空 | `completions_command_writes_to_stdout` |
| P1 | 非 TTY 关闭颜色 | 非终端输出不包含 ANSI 颜色控制序列 | `colors_disabled_when_not_tty` |

---

## JSON 行为（重点）

### 统一结构

- **成功**：
  - `{"ok": true, "data": ...}`
- **失败**：
  - `{"ok": false, "error": {"command": "...", "exit_code": N, "message": "..."}}`

### 契约矩阵

| 级别 | 契约 | 当前行为 | 测试映射 |
|---|---|---|---|
| P0 | `list-ports --json` 必须可解析 JSON | 成功时 `ok=true` 且 `data.ports` 为数组 | `list_ports_json_returns_valid_json`、`json_output_is_valid_json_without_extra_lines` |
| P0 | `info --json` 成功必须可解析 JSON | 成功时 `ok=true` 且 `data` 对象存在 | `info_json_success_returns_structured_json` |
| P0 | `info --json` 失败必须可解析 JSON | 失败时 `ok=false` 且 `error` 字段完整 | `info_json_error_keeps_stdout_clean`、`info_json_error_returns_clean_error_json` |
| P0 | JSON 模式不污染 stderr | 成功/失败路径都要求 stderr 为空（避免影响脚本解析） | 同上 4 个 JSON 测试 |

---

## i18n（帮助与文案）

| 级别 | 契约 | 当前行为 | 测试映射 |
|---|---|---|---|
| P0 | 中英文帮助标题稳定 | zh-CN 与 en 的标题、分区名符合预期 | `test_main_help_zh_cn_has_localized_headings`、`test_main_help_en_has_english_headings` |
| P0 | zh-CN 无英文泄漏 | 关键帮助文本在 zh-CN 输出中不出现英文模板文案 | `test_main_help_no_english_leaks_zh_cn`、`test_subcmd_help_flash_no_english_leaks_zh_cn` |
| P1 | 子命令帮助本地化完整 | 子命令 about/参数说明/全局参数传播 | `test_subcmd_help_flash_zh_cn_has_localized_content`、`test_subcmd_help_flash_global_args_propagated`、`test_all_subcommands_have_localized_about_zh_cn` |
| P1 | 语言回退策略稳定 | locale 变体映射（`zh_* -> zh-CN`, 其他回退 `en`） | `locale_tests::*` |

---

## 非交互与自动化

| 级别 | 契约 | 当前行为 | 测试映射 |
|---|---|---|---|
| P0 | `--non-interactive` 生效 | 多候选不弹窗，快速失败 | `non_interactive_flag_is_recognized`、`non_interactive_flash_with_multiple_firmwares_fails_fast` |
| P0 | `HISIFLASH_NON_INTERACTIVE=true` 生效 | 环境变量可触发同等行为 | `non_interactive_environment_variable_works` |
| P1 | 非交互端口选择错误分类稳定 | 0 个/多个端口映射为 `Usage` 类错误 | `serial::tests::test_select_non_interactive_*` |

---

## 维护规则

当以下行为发生变更时，必须同步更新：

1. 本文档中的对应契约条目（行为描述与级别）；
2. 对应测试用例（新增/修改断言）；
3. `hisiflash-cli/README.md` 中相关兼容性说明（若影响用户可见行为）。

建议在 PR 描述中增加 “CLI Contract Impact” 小节，明确：

- 影响维度（flags / exit / stdout-stderr / json / i18n）
- 兼容性级别（P0/P1/P2）
- 对应测试变更

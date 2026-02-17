# hisiflash-cli

`hisiflash-cli` 是 `hisiflash` 项目的命令行应用 crate，二进制名为 `hisiflash`，面向 HiSilicon 芯片固件烧录与调试流程。

> ✅ CLI 行为已做固化：命令结构、参数语义、交互流程、输出约定与退出码按兼容性策略维护。

## 定位

- 为终端用户提供开箱即用的烧录/擦除/信息查询/监控命令
- 在 `hisiflash` 库能力上封装交互式体验、配置文件与多语言输出
- 兼容自动化场景（非交互、环境变量、JSON 输出）

## 主要命令

- `flash`：烧录 FWPKG 固件包
- `write`：写入多个裸机二进制片段
- `write-program`：写入单个程序二进制
- `erase`：擦除 Flash
- `info`：显示固件包信息
- `list-ports`：列出串口
- `monitor`：串口监控
- `completions`：生成 shell 补全

## 安装

### 从 crates.io 安装

```bash
cargo install hisiflash-cli
```

### 从源码安装

在仓库根目录执行：

```bash
cargo install --path hisiflash-cli
```

### 使用 cargo-binstall（预编译包）

```bash
cargo binstall hisiflash-cli
```

## 快速开始

```bash
# 查看帮助
hisiflash --help

# 列出串口
hisiflash list-ports

# 自动检测串口并烧录
hisiflash flash firmware.fwpkg

# 指定串口烧录
hisiflash flash -p /dev/ttyUSB0 firmware.fwpkg

# 擦除全部（危险操作）
hisiflash erase -p /dev/ttyUSB0 --all

# 串口监控
hisiflash monitor -p /dev/ttyUSB0
```

## 配置与环境变量

支持本地与全局 TOML 配置：

- 本地：`hisiflash.toml` / `hisiflash_ports.toml`
- 全局：`~/.config/hisiflash/config.toml`

常用环境变量：

- `HISIFLASH_PORT`
- `HISIFLASH_BAUD`
- `HISIFLASH_CHIP`
- `HISIFLASH_LANG`
- `HISIFLASH_NON_INTERACTIVE`
- `RUST_LOG`

## 交互与自动化

- 交互式串口选择（多设备场景）
- `--non-interactive`：CI/CD 友好，禁用交互
- `--quiet` / `-v -vv -vvv`：控制输出粒度
- 部分命令支持 `--json` 结构化输出（如 `info`、`list-ports`）

## 开发与验证

在仓库根目录执行：

```bash
cargo run --bin hisiflash -- --help
cargo test -p hisiflash-cli
cargo clippy -p hisiflash-cli --all-targets -- -D warnings
```

更多信息请参考：

- 根 `README.md`
- `hisiflash-cli/src/main.rs`
- `docs/testing/AUTOMATED_TESTS.md`
- `docs/testing/CLI_COMPATIBILITY_MATRIX.md`

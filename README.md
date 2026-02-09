# hisiflash

一个跨平台的 HiSilicon 芯片烧录工具，使用 Rust 编写。灵感来自 [espflash](https://github.com/esp-rs/espflash) 和 [esptool](https://github.com/espressif/esptool)。

[![CI](https://github.com/sanchuanhehe/hisiflash/actions/workflows/ci.yml/badge.svg)](https://github.com/sanchuanhehe/hisiflash/actions/workflows/ci.yml)
[![Rust](https://img.shields.io/badge/rust-1.85%2B-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)

## 特性

### 核心功能
- 🚀 **高性能**：原生 Rust 实现，启动快速
- 🔧 **跨平台**：支持 Linux、macOS、Windows
- 📦 **FWPKG 支持**：完整支持 HiSilicon FWPKG 固件包格式
- 🔄 **SEBOOT 协议**：兼容官方 fbb_burntool 协议
- 🛠️ **库 + CLI**：既可作为库使用，也可作为命令行工具

### 智能检测
- 🔌 **USB VID/PID 自动检测**：支持 CH340/CP210x/FTDI/PL2303/HiSilicon 原生 USB
- 🎯 **交互式串口选择**：多串口时自动提示选择，已知设备高亮显示
- 💾 **串口记忆功能**：可保存常用串口到配置文件

### 用户体验
- 📊 **彩色进度条**：友好的烧录进度显示
- 🔇 **静默模式**：`-q/--quiet` 抑制非必要输出
- 📝 **分级详细模式**：`-v/-vv/-vvv` 三级调试输出
- 🤖 **非交互模式**：`--non-interactive` 支持 CI/CD 环境

### 配置与扩展
- ⚙️ **TOML 配置文件**：支持本地 (`hisiflash.toml`) 和全局 (`~/.config/hisiflash/`) 配置
- 🌍 **环境变量**：完整的环境变量支持 (HISIFLASH_PORT/BAUD/CHIP 等)
- 🐚 **Shell 补全**：支持 Bash/Zsh/Fish/PowerShell 自动补全
- 📡 **串口监控**：内置 `monitor` 命令查看设备输出

## 支持的芯片

| 芯片 | 状态 | 说明 |
|------|------|------|
| WS63 | ✅ 完整支持 | WiFi + BLE +SLE 芯片 |
| BS2X | � 计划中 | BS21 等 BLE + SLE 芯片（使用相同 SEBOOT 协议） |
| BS25 | 📋 计划中 | BLE + SLE 增强版 |

WS63 和 BS2X 系列使用相同的 SEBOOT 烧录协议，BS2X/BS25 支持将在后续版本中添加。

## 安装

### 使用 Cargo 安装（推荐）

```bash
# 从 crates.io 安装
cargo install hisiflash-cli

# 或使用 cargo-binstall 安装预编译二进制（更快）
cargo binstall hisiflash-cli
```

### 从源码编译

```bash
# 克隆仓库
git clone https://github.com/sanchuanhehe/hisiflash.git
cd hisiflash

# 编译 release 版本
cargo build --release

# 安装到系统
cargo install --path hisiflash-cli
```

### 二进制下载

从 [Releases](https://github.com/sanchuanhehe/hisiflash/releases) 页面下载预编译的二进制文件。

### 安装 Shell 补全（可选）

安装后，生成 shell 补全脚本以获得更好的命令行体验：

```bash
# Bash
hisiflash completions bash >> ~/.bashrc

# Zsh (方式一：添加到 .zshrc)
hisiflash completions zsh >> ~/.zshrc

# Zsh (方式二：使用补全目录)
mkdir -p ~/.zfunc
hisiflash completions zsh > ~/.zfunc/_hisiflash
# 确保 ~/.zfunc 在 fpath 中，在 .zshrc 中添加: fpath=(~/.zfunc $fpath)

# Fish
mkdir -p ~/.config/fish/completions
hisiflash completions fish > ~/.config/fish/completions/hisiflash.fish

# PowerShell
hisiflash completions powershell >> $PROFILE
```

重新打开终端或执行 `source ~/.bashrc`（或对应的配置文件）使补全生效。

## 快速开始

### 列出可用串口

```bash
hisiflash list-ports
```

输出示例（自动识别设备类型）：
```
Available Serial Ports
  • /dev/ttyUSB0 [CH340/CH341] (1A86:7523) - USB Serial
→ Auto-detected: /dev/ttyUSB0
```

### 烧录 FWPKG 固件包

```bash
# 自动检测串口
hisiflash flash firmware.fwpkg

# 指定串口
hisiflash flash -p /dev/ttyUSB0 firmware.fwpkg
```

### 使用更高波特率

```bash
hisiflash flash -p /dev/ttyUSB0 -b 921600 firmware.fwpkg
```

### 指定芯片类型

```bash
# WS63 芯片（默认）
hisiflash -c ws63 flash firmware.fwpkg

# BS2X 系列芯片
hisiflash -c bs2x flash firmware.fwpkg
```

### 只烧录指定分区

```bash
hisiflash flash -p /dev/ttyUSB0 --filter "app,nv" firmware.fwpkg
```

### 查看固件信息

```bash
hisiflash info firmware.fwpkg
```

### 写入裸机二进制

```bash
hisiflash write -p /dev/ttyUSB0 \
    --loaderboot loaderboot.bin \
    -B app.bin:0x00800000 \
    -B nv.bin:0x003F0000
```

### 擦除全部 Flash

```bash
hisiflash erase -p /dev/ttyUSB0 --all
```

## 命令行参数

```
hisiflash [OPTIONS] <COMMAND>

Commands:
  flash          烧录 FWPKG 固件包
  write          写入裸机二进制文件
  write-program  写入单个程序二进制
  erase          擦除 Flash
  info           显示固件信息
  list-ports     列出可用串口
  monitor        串口监控
  completions    生成 Shell 补全脚本
  help           显示帮助信息

Options:
  -p, --port <PORT>      串口设备 [env: HISIFLASH_PORT]
  -b, --baud <BAUD>      波特率 [default: 921600] [env: HISIFLASH_BAUD]
  -c, --chip <CHIP>      芯片类型 [default: ws63] [env: HISIFLASH_CHIP]
      --lang <LANG>      语言/地区 (如 en, zh-CN) [env: HISIFLASH_LANG]
  -v, --verbose...       详细输出级别 (-v, -vv, -vvv)
  -q, --quiet            静默模式
      --non-interactive  非交互模式 [env: HISIFLASH_NON_INTERACTIVE]
      --confirm-port     强制确认端口选择
      --list-all-ports   列出所有端口（包括未知类型）
  -h, --help             显示帮助
  -V, --version          显示版本
```

## 环境变量

| 变量 | 说明 | 默认值 |
|------|------|--------|
| `HISIFLASH_PORT` | 默认串口 | - |
| `HISIFLASH_BAUD` | 默认波特率 | 921600 |
| `HISIFLASH_CHIP` | 默认芯片类型 | ws63 |
| `HISIFLASH_LANG` | 语言/地区 (如 en, zh-CN) | 自动检测 |
| `HISIFLASH_NON_INTERACTIVE` | 非交互模式 | false |
| `RUST_LOG` | 日志级别 | info |

## 可靠性与重试机制

hisiflash 内置多层重试机制，确保烧录过程的可靠性：

| 操作 | 重试次数 | 说明 |
|------|---------|------|
| 打开串口 | 3 次 | 串口被占用或设备未就绪时自动重试 |
| 连接握手 | 7 次 | 设备未响应时多次尝试握手 |
| 下载传输 | 3 次 | 数据传输失败时自动重试 |
| YMODEM 块 | 10 次 | 单个数据块传输失败时重试 |

这些参数参考了 esptool 和 espflash 的最佳实践，在大多数情况下无需手动配置。

## 国际化 (i18n)

hisiflash 支持多语言界面：

- **自动检测**：默认自动检测系统语言
- **手动设置**：使用 `--lang` 参数或 `HISIFLASH_LANG` 环境变量

**支持的语言**：

| 语言 | 代码 |
|------|------|
| English | `en` |
| 简体中文 | `zh-CN` |

**使用示例**：

```bash
# 使用英文界面
hisiflash --lang en list-ports

# 使用中文界面
hisiflash --lang zh-CN list-ports

# 通过环境变量设置
export HISIFLASH_LANG=zh-CN
hisiflash list-ports
```

## 配置文件

hisiflash 支持 TOML 格式的配置文件：

**本地配置** (当前目录): `hisiflash.toml` 或 `hisiflash_ports.toml`

**全局配置**: `~/.config/hisiflash/config.toml`

```toml
[port.connection]
serial = "/dev/ttyUSB0"
baud = 921600

[flash]
late_baud = false

# 自定义 USB 设备用于自动检测
[[port.usb_device]]
vid = 0x1A86
pid = 0x7523
```

## Shell 补全

详见 [安装 Shell 补全](#安装-shell-补全可选) 章节。

生成补全脚本的基本命令：

```bash
# 查看支持的 shell
hisiflash completions --help

# 生成指定 shell 的补全脚本
hisiflash completions <bash|zsh|fish|powershell|elvish>

# PowerShell
hisiflash completions powershell > _hisiflash.ps1
```

## 作为库使用

添加依赖到 `Cargo.toml`:

```toml
[dependencies]
hisiflash = "0.1"
```

示例代码:

```rust
use hisiflash::{ChipFamily, Fwpkg};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 解析固件包
    let fwpkg = Fwpkg::from_file("firmware.fwpkg")?;
    
    // 创建烧录器并连接
    let chip = ChipFamily::Ws63;
    let mut flasher = chip.create_flasher("/dev/ttyUSB0", 921600, false, 0)?;
    flasher.connect()?;
    
    // 烧录固件
    flasher.flash_fwpkg(&fwpkg, None, &mut |name, current, total| {
        println!("Flashing {}: {}/{}", name, current, total);
    })?;
    
    // 复位设备
    flasher.reset()?;
    
    Ok(())
}
```

## 项目结构

```
hisiflash/
├── Cargo.toml              # Workspace 配置
├── README.md               # 本文件
├── docs/                   # 文档
│   ├── REQUIREMENTS.md     # 需求文档
│   ├── ARCHITECTURE.md     # 架构设计
│   ├── COMPARISON.md       # 功能对比分析
│   └── protocols/          # 协议文档
│       └── PROTOCOL.md     # SEBOOT 协议规范
├── hisiflash/              # 核心库
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── error.rs        # 错误类型
│       ├── connection/     # 连接抽象 (旧版)
│       │   ├── mod.rs
│       │   ├── serial.rs
│       │   └── detect.rs   # USB VID/PID 检测
│       ├── port/           # Port 抽象 (跨平台)
│       │   ├── mod.rs
│       │   ├── native.rs
│       │   └── wasm.rs
│       ├── protocol/       # 协议实现
│       │   ├── mod.rs
│       │   ├── seboot.rs   # SEBOOT 协议
│       │   ├── crc.rs      # CRC16-XMODEM
│       │   └── ymodem.rs   # YMODEM-1K
│       ├── image/          # 固件格式
│       │   ├── mod.rs
│       │   └── fwpkg.rs    # FWPKG 解析
│       └── target/         # 芯片支持
│           ├── mod.rs
│           ├── chip.rs     # Flasher trait
│           └── ws63/
│               ├── mod.rs
│               ├── protocol.rs
│               └── flasher.rs
└── hisiflash-cli/          # CLI 工具
    ├── Cargo.toml
    ├── locales/            # 国际化翻译
    │   ├── en.yml
    │   └── zh-CN.yml
    └── src/
        ├── main.rs
        ├── config.rs       # 配置文件支持
        ├── serial.rs       # 交互式串口选择
        └── commands/
            └── mod.rs      # 预留模块
```

## 开发

### 构建

```bash
cargo build
```

### 测试

```bash
cargo test
```

### 格式化

```bash
cargo fmt
```

### Lint

```bash
cargo clippy
```

## 协议参考

本项目参考了以下开源项目的协议实现：

- [ws63flash](https://github.com/example/ws63flash) - WS63 协议逆向工程
- [espflash](https://github.com/esp-rs/espflash) - Rust 架构参考
- [esptool](https://github.com/espressif/esptool) - 功能参考

## 许可证

本项目采用双许可证：

- MIT License
- Apache License 2.0

详见 [LICENSE-MIT](LICENSE-MIT) 和 [LICENSE-APACHE](LICENSE-APACHE)。

## 致谢

感谢所有参考项目的贡献者们！

## 贡献

欢迎提交 Issue 和 Pull Request！

# hisiflash

一个跨平台的 HiSilicon 芯片烧录工具，使用 Rust 编写。灵感来自 [espflash](https://github.com/esp-rs/espflash) 和 [esptool](https://github.com/espressif/esptool)。

[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)

## 特性

- 🚀 **高性能**：原生 Rust 实现，启动快速
- 🔧 **跨平台**：支持 Linux、macOS、Windows
- 📦 **FWPKG 支持**：完整支持 HiSilicon FWPKG 固件包格式
- 🔌 **智能检测**：通过 USB VID/PID 自动检测串口设备（支持 CH340/CP210x/FTDI）
- 📊 **进度显示**：友好的烧录进度条
- 🛠️ **库 + CLI**：既可作为库使用，也可作为命令行工具
- 🔄 **SEBOOT 协议**：兼容官方 fbb_burntool 协议

## 支持的芯片

| 芯片 | 状态 | 说明 |
|------|------|------|
| WS63 | ✅ 完整支持 | WiFi + BLE 芯片 |
| BS2X | 🔨 开发中 | BS21 等 BLE 芯片 |
| BS25 | 🔨 开发中 | BLE 增强版 |

WS63 和 BS2X 系列使用相同的 SEBOOT 烧录协议。

## 安装

### 从源码编译

```bash
# 克隆仓库
git clone https://github.com/example/hisiflash.git
cd hisiflash

# 编译 release 版本
cargo build --release

# 安装到系统
cargo install --path hisiflash-cli
```

### 二进制下载

从 [Releases](https://github.com/example/hisiflash/releases) 页面下载预编译的二进制文件。

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
  help           显示帮助信息

Options:
  -p, --port <PORT>      串口设备 [env: HISIFLASH_PORT]
  -b, --baud <BAUD>      波特率 [default: 921600] [env: HISIFLASH_BAUD]
  -c, --chip <CHIP>      芯片类型 [default: ws63] [env: HISIFLASH_CHIP]
  -v, --verbose...       详细输出级别 (-v, -vv, -vvv)
  -h, --help             显示帮助
  -V, --version          显示版本
```

## 环境变量

| 变量 | 说明 | 默认值 |
|------|------|--------|
| `HISIFLASH_PORT` | 默认串口 | - |
| `HISIFLASH_BAUD` | 默认波特率 | 921600 |
| `HISIFLASH_CHIP` | 默认芯片类型 | ws63 |

## 作为库使用

添加依赖到 `Cargo.toml`:

```toml
[dependencies]
hisiflash = "0.1"
```

示例代码:

```rust
use hisiflash::{Ws63Flasher, Fwpkg};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 解析固件包
    let fwpkg = Fwpkg::from_file("firmware.fwpkg")?;
    
    // 创建烧录器并连接
    let mut flasher = Ws63Flasher::new("/dev/ttyUSB0", 921600)?;
    flasher.connect()?;
    
    // 烧录固件
    flasher.flash_fwpkg(&fwpkg, None, |name, current, total| {
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
│   └── protocols/          # 协议文档
│       └── WS63_PROTOCOL.md
├── hisiflash/              # 核心库
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── error.rs        # 错误类型
│       ├── connection/     # 连接抽象
│       │   ├── mod.rs
│       │   └── serial.rs
│       ├── protocol/       # 协议实现
│       │   ├── mod.rs
│       │   ├── crc.rs      # CRC16-XMODEM
│       │   └── ymodem.rs   # YMODEM-1K
│       ├── image/          # 固件格式
│       │   ├── mod.rs
│       │   └── fwpkg.rs    # FWPKG 解析
│       └── target/         # 芯片支持
│           ├── mod.rs
│           └── ws63/
│               ├── mod.rs
│               ├── protocol.rs
│               └── flasher.rs
└── hisiflash-cli/          # CLI 工具
    ├── Cargo.toml
    └── src/
        ├── main.rs
        └── commands/
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

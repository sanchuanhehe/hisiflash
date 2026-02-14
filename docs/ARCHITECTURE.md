# HisiFlash 项目结构设计

## 目录结构

```
hisiflash/
├── Cargo.toml                    # 工作空间配置
├── README.md                     # 项目说明
├── CHANGELOG.md                  # 变更日志
├── CONTRIBUTING.md               # 贡献指南
├── rustfmt.toml                  # 代码格式化配置
├── .gitignore
│
├── docs/                         # 文档目录
│   ├── REQUIREMENTS.md           # 需求规格说明书
│   ├── ARCHITECTURE.md           # 架构设计文档 (本文件)
│   ├── COMPARISON.md             # 功能对比文档
│   └── protocols/                # 协议文档
│       └── PROTOCOL.md           # SEBOOT 协议规范 (HiSilicon + YMODEM + FWPKG)
│
├── hisiflash/                    # 核心库 crate
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs                # 库入口
│       ├── error.rs              # 错误定义
│       │
│       ├── device/               # 设备发现与分类模块
│       │   └── mod.rs            # 端点发现 + USB VID/PID 分类
│       │
│       ├── port/                 # Port 传输抽象 (跨平台)
│       │   ├── mod.rs            # Port trait 定义
│       │   ├── native.rs         # 原生串口 (Linux/macOS/Windows)
│       │   └── wasm.rs           # WASM/Web Serial API (实验性)
│       │
│       ├── target/               # 目标芯片模块
│       │   ├── mod.rs
│       │   ├── chip.rs           # 芯片类型抽象 + Flasher trait
│       │   └── ws63/             # WS63 芯片实现
│       │       ├── mod.rs
│       │       ├── flasher.rs    # WS63 烧写器 (含重试机制)
│       │       └── protocol.rs   # WS63 命令帧构建
│       │
│       ├── protocol/             # 传输协议模块
│       │   ├── mod.rs
│       │   ├── seboot.rs         # HiSilicon SEBOOT 官方协议
│       │   ├── ymodem.rs         # YMODEM-1K 协议
│       │   └── crc.rs            # CRC16-XMODEM
│       │
│       └── image/                # 镜像处理模块
│           ├── mod.rs
│           └── fwpkg.rs          # FWPKG 格式解析 (V1 + V2)
│
└── hisiflash-cli/                # CLI 工具 crate
    ├── Cargo.toml
    ├── locales/                  # 国际化翻译文件
    │   ├── en.yml                # 英文
    │   └── zh-CN.yml             # 简体中文
    └── src/
        ├── main.rs               # CLI 入口 + 所有子命令实现
        ├── config.rs             # TOML 配置文件加载/保存
        ├── serial.rs             # 交互式串口选择
        └── commands/             # 子命令模块 (预留)
            └── mod.rs
```

## 支持的芯片系列

| 芯片系列 | 状态 | 协议 | 说明 |
|---------|------|------|------|
| WS63 | ✅ 支持 | SEBOOT | WiFi + BLE, 主要目标 |
| BS2X | 🔨 计划中 | SEBOOT | BS21 等, 纯 BLE |
| BS25 | 🔨 计划中 | SEBOOT | BLE 增强版 |
| WS53 | 📋 规划中 | SEBOOT | WiFi + BLE |

## 串口自动检测

hisiflash 支持通过 USB VID/PID 自动检测开发板串口:

| 设备类型 | VID | PID | 说明 |
|---------|-----|-----|------|
| CH340/CH341 | 0x1A86 | 0x7523/0x5523/0x55D4 | 常见 USB 转串口 |
| CP210x | 0x10C4 | 0xEA60/0xEA70/0xEA71 | Silicon Labs |
| FTDI | 0x0403 | 0x6001/0x6010/等 | FT232/FT2232 |
| HiSilicon | 0x12D1 | * | 原生 USB 设备 |

## 中断传播与取消模型

hisiflash 采用“显式取消上下文 + 全局原子标志”的中断传播模型：

### 架构演进

| 阶段 | 模型 | 特点 |
|------|------|------|
| v0.1.x | 全局 OnceLock | `INTERRUPT_CHECKER` 全局变量，隐式依赖 |
| v0.2.0+ | 原子标志 | `CancelContext` 参数化依赖，可组合可测试 |

### 核心 API

```rust
// hisiflash/src/lib.rs

/// 取消上下文 - 用于检查操作是否被用户中断
pub struct CancelContext {
    checker: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
}

impl CancelContext {
    /// 创建新的取消上下文（自定义检查器）
    pub fn new<F>(checker: F) -> Self
    where
        F: Fn() -> bool + Send + Sync + 'static;

    /// 创建无操作的取消上下文（不响应中断）
    pub fn none() -> Self;

    /// 检查是否已中断，若是则返回错误
    pub fn check(&self) -> Result<()>;
}

/// 从全局中断检查器创建取消上下文（向后兼容）
pub fn cancel_context_from_global() -> CancelContext;
```

### 使用模式

**1. 库内部（原子标志）**

```rust
// hisiflash/src/target/ws63/flasher.rs

pub struct Ws63Flasher<P: Port> {
    port: P,
    cancel: CancelContext,  // 持有取消上下文
}

impl<P: Port> Ws63Flasher<P> {
    pub fn new_with_cancel(port: P, target_baud: u32, cancel: CancelContext) -> Self;
}
```

**2. 原生实现（全局桥接）**

```rust
// hisiflash/src/target/ws63/flasher.rs - native_impl

impl Ws63Flasher<NativePort> {
    pub fn open(port_name: &str, target_baud: u32) -> Result<Self> {
        // 使用全局桥接，自动接入 CLI 设置的中断标志
        Self::with_cancel(
            port,
            target_baud,
            crate::cancel_context_from_global(),
        )
    }
}
```

**3. CLI 端（全局注册）**

```rust
// hisiflash-cli/src/main.rs

fn main() {
    // 注册全局中断检查器
    hisiflash::set_interrupt_flag();

    // ... 执行命令
}
```

### 设计优势

| 方面 | 说明 |
|------|------|
| **可测试性** | 可注入自定义取消检查器，无需修改全局状态 |
| **可组合性** | 多个 Flasher 实例可使用不同的取消策略 |
| **清晰依赖** | 取消语义从隐式变为显式，易于理解和维护 |
| **向后兼容** | `cancel_context_from_global()` 保留全局行为 |

### 中断传播流程

```
┌─────────────────────────────────────────────────────────────────┐
│  CLI 捕获 SIGINT                                                │
│       ↓                                                        │
│  设置原子标志 INTERRUPT_FLAG = true                             │
│       ↓                                                        │
│  库内循环调用 cancel.check()                                     │
│       ↓                                                        │
│  返回 Error::Io(ErrorKind::Interrupted)                        │
│       ↓                                                        │
│  短路后续重试，快速返回                                         │
└─────────────────────────────────────────────────────────────────┘
```

该模型确保：

- **一致性**：不同命令/阶段共享同一取消语义。
- **快速响应**：避免“已按 Ctrl-C 但仍要等待整个超时/重试轮次”。
- **安全性**：在数据传输阶段中断时尽快停止后续写入动作。

## Phase 1: WS63 核心数据结构

### FWPKG 固件包格式

```rust
// image/fwpkg.rs

/// FWPKG 文件头 (12 字节)
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct FwpkgHeader {
    /// 魔数: 0xDFADBEEF (小端存储, 读取时为 0xEFBEADDF)
    pub magic: u32,
    /// CRC16-XMODEM 校验 (从 cnt 字段开始)
    pub crc: u16,
    /// 分区数量 (最大 16)
    pub cnt: u16,
    /// 固件总大小
    pub len: u32,
}

impl FwpkgHeader {
    pub const MAGIC: u32 = 0xEFBEADDF;
    pub const MAX_PARTITIONS: usize = 16;
    
    pub fn is_valid(&self) -> bool {
        self.magic == Self::MAGIC && (self.cnt as usize) <= Self::MAX_PARTITIONS
    }
}

/// FWPKG 分区信息 (56 字节)
#[repr(C, packed)]
#[derive(Debug, Clone)]
pub struct FwpkgBinInfo {
    /// 文件名 (最大 31 字符 + NUL)
    pub name: [u8; 32],
    /// 在 fwpkg 中的偏移
    pub offset: u32,
    /// 文件长度
    pub length: u32,
    /// 烧写地址
    pub burn_addr: u32,
    /// 烧写大小
    pub burn_size: u32,
    /// 类型: 0=loaderboot, 1=普通固件
    pub type_2: u32,
}

impl FwpkgBinInfo {
    /// 是否为 LoaderBoot
    pub fn is_loaderboot(&self) -> bool {
        self.type_2 == 0
    }
    
    /// 获取文件名字符串
    pub fn name_str(&self) -> &str {
        let end = self.name.iter().position(|&c| c == 0).unwrap_or(32);
        std::str::from_utf8(&self.name[..end]).unwrap_or("")
    }
}

/// 解析后的 FWPKG 固件包
pub struct Fwpkg {
    pub header: FwpkgHeader,
    pub bins: Vec<FwpkgBinInfo>,
    data: Vec<u8>,
}

impl Fwpkg {
    /// 从文件加载 FWPKG
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self>;
    
    /// 获取 LoaderBoot
    pub fn loaderboot(&self) -> Option<&FwpkgBinInfo>;
    
    /// 获取指定分区的数据
    pub fn bin_data(&self, bin: &FwpkgBinInfo) -> &[u8];
    
    /// 获取所有普通分区 (type_2 == 1)
    pub fn normal_bins(&self) -> impl Iterator<Item = &FwpkgBinInfo>;
    
    /// 验证 CRC
    pub fn verify_crc(&self) -> bool;
}
```

### WS63 命令帧协议

```rust
// target/ws63/protocol.rs

/// WS63 帧魔数
pub const FRAME_MAGIC: u32 = 0xDEADBEEF;

/// 命令类型
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandType {
    /// 握手命令
    Handshake = 0xF0,
    /// 设置波特率
    SetBaudRate = 0x5A,
    /// 下载/擦除
    Download = 0xD2,
    /// 复位
    Reset = 0x87,
}

impl CommandType {
    /// 获取交换后的命令码 (SCMD)
    pub fn swapped(self) -> u8 {
        let cmd = self as u8;
        (cmd << 4) | (cmd >> 4)
    }
}

/// 命令帧构建器
pub struct CommandFrame {
    cmd: CommandType,
    data: Vec<u8>,
}

impl CommandFrame {
    pub fn new(cmd: CommandType) -> Self {
        Self { cmd, data: Vec::new() }
    }
    
    /// 握手命令
    pub fn handshake(baud: u32) -> Self {
        let mut frame = Self::new(CommandType::Handshake);
        frame.data.extend_from_slice(&baud.to_le_bytes());
        frame.data.extend_from_slice(&0x0108u32.to_le_bytes()); // Magic
        frame
    }
    
    /// 设置波特率命令
    pub fn set_baud_rate(baud: u32) -> Self {
        let mut frame = Self::new(CommandType::SetBaudRate);
        frame.data.extend_from_slice(&baud.to_le_bytes());
        frame.data.extend_from_slice(&0x0108u32.to_le_bytes());
        frame
    }
    
    /// 下载命令
    pub fn download(addr: u32, len: u32, erase_size: u32) -> Self {
        let mut frame = Self::new(CommandType::Download);
        frame.data.extend_from_slice(&addr.to_le_bytes());
        frame.data.extend_from_slice(&len.to_le_bytes());
        frame.data.extend_from_slice(&erase_size.to_le_bytes());
        frame.data.extend_from_slice(&[0x00, 0xFF]); // Const
        frame
    }
    
    /// 擦除全部 Flash
    pub fn erase_all() -> Self {
        Self::download(0, 0, 0xFFFFFFFF)
    }
    
    /// 复位命令
    pub fn reset() -> Self {
        let mut frame = Self::new(CommandType::Reset);
        frame.data.extend_from_slice(&[0x00, 0x00]);
        frame
    }
    
    /// 构建完整的帧数据
    pub fn build(&self) -> Vec<u8> {
        let total_len = 10 + self.data.len(); // Magic(4) + Len(2) + CMD(1) + SCMD(1) + Data + CRC(2)
        let mut buf = Vec::with_capacity(total_len);
        
        // Magic
        buf.extend_from_slice(&FRAME_MAGIC.to_le_bytes());
        // Length
        buf.extend_from_slice(&(total_len as u16).to_le_bytes());
        // CMD + SCMD
        buf.push(self.cmd as u8);
        buf.push(self.cmd.swapped());
        // Data
        buf.extend_from_slice(&self.data);
        // CRC (计算前面所有数据)
        let crc = crc16_xmodem(&buf);
        buf.extend_from_slice(&crc.to_le_bytes());
        
        buf
    }
}

/// 应答帧解析
pub struct ResponseFrame {
    pub cmd: u8,
    pub data: Vec<u8>,
}

impl ResponseFrame {
    /// 握手成功的应答
    pub const HANDSHAKE_ACK: &'static [u8] = &[
        0xEF, 0xBE, 0xAD, 0xDE, // Magic
        0x0C, 0x00,             // Length = 12
        0xE1, 0x1E,             // CMD = 0xE1, SCMD = 0x1E
        0x5A, 0x00,             // ACK = 0x5A
    ];
    
    /// 检查是否为握手成功应答
    pub fn is_handshake_ack(data: &[u8]) -> bool {
        data.windows(10).any(|w| w == Self::HANDSHAKE_ACK)
    }
}
```

### YMODEM 协议

```rust
// protocol/ymodem.rs

/// YMODEM 控制字符
pub mod control {
    pub const SOH: u8 = 0x01;  // 128 字节包头
    pub const STX: u8 = 0x02;  // 1024 字节包头
    pub const EOT: u8 = 0x04;  // 传输结束
    pub const ACK: u8 = 0x06;  // 确认
    pub const NAK: u8 = 0x15;  // 否定确认
    pub const C: u8 = b'C';    // CRC 模式请求
}

/// YMODEM 传输器
pub struct YmodemTransfer<'a, P: Port> {
    port: &'a mut P,
    verbose: u8,
}

impl<'a, P: Port> YmodemTransfer<'a, P> {
    pub fn new(port: &'a mut P, verbose: u8) -> Self {
        Self { port, verbose }
    }
    
    /// 等待接收方发送 'C'
    pub fn wait_for_c(&mut self, timeout: Duration) -> Result<()>;
    
    /// 发送文件信息包 (Block 0)
    pub fn send_file_info(&mut self, filename: &str, filesize: usize) -> Result<()>;
    
    /// 发送数据块
    pub fn send_data_block(&mut self, seq: u8, data: &[u8]) -> Result<()>;
    
    /// 发送 EOT 并等待 ACK
    pub fn send_eot(&mut self) -> Result<()>;
    
    /// 发送结束包 (空 Block 0)
    pub fn send_finish(&mut self) -> Result<()>;
    
    /// 传输文件
    pub fn transfer_file<R, F>(
        &mut self, 
        filename: &str, 
        data: R,
        progress: Option<F>,
    ) -> Result<()>
    where
        R: Read,
        F: FnMut(usize, usize);
}
```

### CRC16-XMODEM

```rust
// protocol/crc.rs

/// CRC16-XMODEM 查找表
const CRC16_TABLE: [u16; 256] = [
    0x0000, 0x1021, 0x2042, 0x3063, 0x4084, 0x50a5, 0x60c6, 0x70e7,
    0x8108, 0x9129, 0xa14a, 0xb16b, 0xc18c, 0xd1ad, 0xe1ce, 0xf1ef,
    // ... 完整表格
];

/// 计算 CRC16-XMODEM
pub fn crc16_xmodem(data: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for &byte in data {
        crc = (crc << 8) ^ CRC16_TABLE[((crc >> 8) ^ (byte as u16)) as usize & 0xFF];
    }
    crc
}
```

### WS63 Flasher

```rust
// flasher/mod.rs

/// WS63 烧写器
pub struct Ws63Flasher {
    port: NativePort,
    baud: u32,
    late_baud: bool,
    verbose: u8,
}

impl Ws63Flasher {
    /// 创建烧写器
    pub fn new(port_name: &str, baud: u32) -> Result<Self>;
    
    /// 设置延迟切换波特率模式
    pub fn with_late_baud(mut self, late_baud: bool) -> Self;
    
    /// 设置详细输出级别
    pub fn with_verbose(mut self, verbose: u8) -> Self;
    
    /// 等待设备复位并握手
    pub fn connect(&mut self) -> Result<()>;
    
    /// 烧写 FWPKG 固件包
    pub fn flash_fwpkg<F>(&mut self, fwpkg: &Fwpkg, filter: Option<&[&str]>, progress: F) -> Result<()>
    where F: FnMut(&str, usize, usize);
    
    /// 烧写裸机二进制
    pub fn write_bins(&mut self, loaderboot: &[u8], bins: &[(&[u8], u32)]) -> Result<()>;
    
    /// 擦除全部 Flash
    pub fn erase_all(&mut self) -> Result<()>;
    
    /// 复位设备
    pub fn reset(&mut self) -> Result<()>;
    
    // 内部方法
    fn send_command(&mut self, frame: &CommandFrame) -> Result<()>;
    fn wait_for_magic(&mut self) -> Result<Vec<u8>>;
    fn ymodem_transfer(&mut self, filename: &str, data: &[u8]) -> Result<()>;
}
```

## 核心库设计 (hisiflash)

### 公开 API 概览

```rust
// lib.rs
pub mod device;
pub mod port;
pub mod target;
pub mod protocol;
pub mod image;
pub mod error;
pub mod host;

// Re-exports
pub use error::{Error, Result};
pub use device::{DetectedPort, DeviceKind, TransportKind};
pub use port::{Port, PortEnumerator, PortInfo, SerialConfig};
pub use target::{ChipFamily, ChipOps, Flasher};
pub use host::{discover_ports, discover_hisilicon_ports, auto_detect_port};
```

### 主要 Traits

#### Port - 传输抽象

```rust
// port/mod.rs
use std::time::Duration;
use crate::Result;

/// 统一传输端口抽象 trait
pub trait Port: Read + Write + Send {
    /// 设置读写超时
    fn set_timeout(&mut self, timeout: Duration) -> Result<()>;

    /// 获取当前超时
    fn timeout(&self) -> Duration;

    /// 设置波特率
    fn set_baud_rate(&mut self, baud: u32) -> Result<()>;

    /// 获取波特率
    fn baud_rate(&self) -> u32;

    /// 清理缓冲区
    fn clear_buffers(&mut self) -> Result<()>;

    /// 端口名称
    fn name(&self) -> &str;

    /// 控制线
    fn set_dtr(&mut self, level: bool) -> Result<()>;
    fn set_rts(&mut self, level: bool) -> Result<()>;

    /// 关闭端口
    fn close(&mut self) -> Result<()>;
}
```

#### ChipTarget - 芯片抽象

```rust
// target/traits.rs
use crate::{Connection, Result};

/// Flash 布局信息
pub struct FlashLayout {
    pub base_address: u32,
    pub size: u32,
    pub sector_size: u32,
    pub page_size: u32,
}

/// 内存映射
pub struct MemoryMap {
    pub ram_start: u32,
    pub ram_size: u32,
    pub flash_start: u32,
    pub flash_size: u32,
}

/// 芯片目标 trait
pub trait ChipTarget: Send + Sync {
    /// 芯片名称
    fn name(&self) -> &'static str;
    
    /// 芯片型号
    fn chip_type(&self) -> ChipType;
    
    /// 芯片 ID
    fn chip_id(&self) -> u32;
    
    /// Flash 布局
    fn flash_layout(&self) -> FlashLayout;
    
    /// 内存映射
    fn memory_map(&self) -> MemoryMap;
    
    /// 默认波特率
    fn default_baud_rate(&self) -> u32 { 115200 }
    
    /// 最大波特率
    fn max_baud_rate(&self) -> u32 { 921600 }
    
    /// 连接握手序列
    fn handshake(&self, conn: &mut Connection) -> Result<()>;
    
    /// 检测芯片
    fn detect(conn: &mut Connection) -> Result<Box<dyn ChipTarget>> 
    where Self: Sized;
    
    /// 进入烧写模式
    fn enter_flash_mode(&self, conn: &mut Connection) -> Result<()>;
    
    /// 退出烧写模式
    fn exit_flash_mode(&self, conn: &mut Connection) -> Result<()>;
    
    /// 烧写前置操作
    fn pre_flash(&self, conn: &mut Connection) -> Result<()> { Ok(()) }
    
    /// 烧写后置操作
    fn post_flash(&self, conn: &mut Connection) -> Result<()> { Ok(()) }
    
    /// 支持的协议
    fn supported_protocols(&self) -> &[ProtocolType];
    
    /// 读取芯片信息
    fn read_chip_info(&self, conn: &mut Connection) -> Result<ChipInfo>;
}
```

#### TransferProtocol - 传输协议抽象

```rust
// protocol/mod.rs
use std::path::Path;
use crate::{Port, Result};

/// 传输进度回调
pub type ProgressCallback = Box<dyn Fn(u64, u64) + Send>;

/// 传输协议 trait
pub trait TransferProtocol: Send {
    /// 协议名称
    fn name(&self) -> &'static str;
    
    /// 初始化协议
    fn init(&mut self, port: &mut dyn Port) -> Result<()>;
    
    /// 发送数据块
    fn send_block(&mut self, port: &mut dyn Port, data: &[u8]) -> Result<()>;
    
    /// 接收数据块
    fn receive_block(&mut self, port: &mut dyn Port) -> Result<Vec<u8>>;
    
    /// 发送文件
    fn send_file<P: AsRef<Path>>(
        &mut self, 
        port: &mut dyn Port,
        path: P,
        progress: Option<ProgressCallback>
    ) -> Result<()>;
    
    /// 接收文件
    fn receive_file<P: AsRef<Path>>(
        &mut self,
        port: &mut dyn Port,
        path: P,
        progress: Option<ProgressCallback>
    ) -> Result<()>;
    
    /// 结束传输
    fn finish(&mut self, port: &mut dyn Port) -> Result<()>;
    
    /// 取消传输
    fn cancel(&mut self, port: &mut dyn Port) -> Result<()>;
}
```

#### FirmwareImage - 固件镜像抽象

```rust
// image/mod.rs
use std::path::Path;
use crate::Result;

/// 镜像段
pub struct ImageSegment {
    pub address: u32,
    pub data: Vec<u8>,
    pub name: Option<String>,
}

/// 固件镜像 trait
pub trait FirmwareImage: Send {
    /// 镜像格式名称
    fn format_name(&self) -> &'static str;
    
    /// 从文件加载
    fn load<P: AsRef<Path>>(path: P) -> Result<Self> where Self: Sized;
    
    /// 从字节加载
    fn from_bytes(data: &[u8]) -> Result<Self> where Self: Sized;
    
    /// 获取所有段
    fn segments(&self) -> &[ImageSegment];
    
    /// 获取入口地址
    fn entry_point(&self) -> Option<u32>;
    
    /// 获取镜像版本
    fn version(&self) -> Option<&str>;
    
    /// 获取镜像描述
    fn description(&self) -> Option<&str>;
    
    /// 合并镜像
    fn merge(&mut self, other: &dyn FirmwareImage) -> Result<()>;
    
    /// 导出为二进制
    fn to_binary(&self) -> Result<Vec<u8>>;
    
    /// 计算校验和
    fn checksum(&self) -> u32;
}
```

### Flasher - 烧写器

```rust
// flasher/mod.rs
use crate::{
    Connection, ChipTarget, FirmwareImage, TransferProtocol,
    FlashSettings, Result
};

/// 进度回调
pub trait ProgressCallbacks: Send {
    fn init(&mut self, total_size: u64);
    fn update(&mut self, current: u64);
    fn finish(&mut self);
}

/// 烧写器
pub struct Flasher {
    connection: Connection,
    chip: Box<dyn ChipTarget>,
    protocol: Box<dyn TransferProtocol>,
    settings: FlashSettings,
}

impl Flasher {
    /// 创建烧写器
    pub fn new(
        connection: Connection,
        chip: Box<dyn ChipTarget>,
        protocol: Box<dyn TransferProtocol>,
        settings: FlashSettings,
    ) -> Self;
    
    /// 自动检测芯片并创建
    pub fn detect(connection: Connection) -> Result<Self>;
    
    /// 连接设备
    pub fn connect(&mut self) -> Result<()>;
    
    /// 断开连接
    pub fn disconnect(&mut self) -> Result<()>;
    
    /// 获取设备信息
    pub fn device_info(&mut self) -> Result<DeviceInfo>;
    
    /// 烧写固件
    pub fn flash(
        &mut self,
        image: &dyn FirmwareImage,
        progress: Option<&mut dyn ProgressCallbacks>,
    ) -> Result<()>;
    
    /// 烧写到指定地址
    pub fn flash_to_address(
        &mut self,
        data: &[u8],
        address: u32,
        progress: Option<&mut dyn ProgressCallbacks>,
    ) -> Result<()>;
    
    /// 读取 Flash
    pub fn read_flash(
        &mut self,
        address: u32,
        size: u32,
        progress: Option<&mut dyn ProgressCallbacks>,
    ) -> Result<Vec<u8>>;
    
    /// 擦除 Flash
    pub fn erase_flash(&mut self, address: u32, size: u32) -> Result<()>;
    
    /// 擦除全部
    pub fn erase_all(&mut self) -> Result<()>;
    
    /// 校验
    pub fn verify(
        &mut self,
        image: &dyn FirmwareImage,
        progress: Option<&mut dyn ProgressCallbacks>,
    ) -> Result<bool>;
    
    /// 复位设备
    pub fn reset(&mut self) -> Result<()>;
    
    /// 读取 eFuse
    pub fn read_efuse(&mut self, address: u32, size: u32) -> Result<Vec<u8>>;
    
    /// 写入 eFuse (危险操作)
    pub fn write_efuse(&mut self, address: u32, data: &[u8]) -> Result<()>;
}
```

## CLI 设计 (hisiflash-cli)

### 命令行参数结构

```rust
// args.rs
use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(name = "hisiflash")]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// 串口设备
    #[arg(short, long, env = "HISIFLASH_PORT")]
    pub port: Option<String>,
    
    /// 波特率
    #[arg(short, long, default_value = "115200", env = "HISIFLASH_BAUD")]
    pub baud: u32,
    
    /// 芯片类型
    #[arg(short, long)]
    pub chip: Option<ChipType>,
    
    /// 配置文件
    #[arg(short = 'C', long)]
    pub config: Option<PathBuf>,
    
    /// 详细输出
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub verbose: u8,
    
    /// 安静模式
    #[arg(short, long)]
    pub quiet: bool,
    
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// 烧写固件到设备
    Flash(FlashArgs),
    /// 从设备读取 Flash 内容
    Read(ReadArgs),
    /// 擦除 Flash
    Erase(EraseArgs),
    /// 显示设备/固件信息
    Info(InfoArgs),
    /// 串口监控
    Monitor(MonitorArgs),
    /// eFuse 操作
    Efuse(EfuseArgs),
    /// 复位设备
    Reset(ResetArgs),
    /// 生成 shell 补全脚本
    Completions(CompletionsArgs),
}

#[derive(Args)]
pub struct FlashArgs {
    /// 固件文件
    pub file: PathBuf,
    
    /// 烧写地址
    #[arg(short, long, value_parser = parse_hex)]
    pub address: Option<u32>,
    
    /// 擦除模式
    #[arg(short, long, default_value = "normal")]
    pub erase: EraseMode,
    
    /// 跳过校验
    #[arg(short = 'n', long)]
    pub no_verify: bool,
    
    /// 烧写后不复位
    #[arg(short = 'r', long)]
    pub no_reset: bool,
    
    /// 只烧写指定分区
    #[arg(long)]
    pub partition: Option<String>,
}
```

## 依赖项

### hisiflash (核心库)

```toml
[dependencies]
# 序列化
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
toml = "0.8"

# 错误处理
thiserror = "1.0"

# 日志
log = "0.4"

# 串口
serialport = "4.3"

# USB
rusb = { version = "0.9", optional = true }

# 字节处理
byteorder = "1.5"
crc = "3.0"

# 异步 (可选)
tokio = { version = "1.0", features = ["rt", "sync"], optional = true }

[features]
default = ["serial"]
serial = ["serialport"]
usb = ["rusb"]
tcp = []
async = ["tokio"]
all = ["serial", "usb", "tcp"]
```

### hisiflash-cli

```toml
[dependencies]
hisiflash = { path = "../hisiflash" }

# CLI
clap = { version = "4.4", features = ["derive", "env"] }
clap_complete = "4.4"

# 日志
env_logger = "0.11"
log = "0.4"

# UI
indicatif = "0.17"
console = "0.15"
comfy-table = "7.1"

# 配置
directories = "5.0"

# 错误处理
miette = { version = "7.0", features = ["fancy"] }
```

## Feature Flags 设计

```toml
[features]
# 默认特性
default = ["cli", "serial"]

# CLI 模块 (包含 clap, indicatif 等)
cli = ["clap", "clap_complete", "indicatif", "console"]

# 连接方式
serial = ["serialport"]
usb = ["rusb"]
tcp = []

# 协议
ymodem = []
xmodem = []

# 芯片支持
chip-wifi5gnb = []
chip-luofu = []
chip-xiling = []
chip-emei = []
chip-all = ["chip-wifi5gnb", "chip-luofu", "chip-xiling", "chip-emei"]

# 异步支持
async = ["tokio"]

# 所有特性
full = ["cli", "serial", "usb", "tcp", "chip-all", "async"]
```

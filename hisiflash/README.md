# hisiflash

`hisiflash` 是 `hisiflash` 工作区中的核心库 crate，提供 HiSilicon 芯片烧录所需的协议实现、端口抽象、设备发现、镜像解析与芯片能力封装。

> ⚠️ 当前版本尚未达到 v1.0.0，对外 API 仍可能调整，暂不承诺接口稳定性。

## 定位

- 提供可复用的 Rust API（供 CLI 或其他上层工具调用）
- 负责底层通信与烧录流程能力，不直接承担命令行交互
- 当前以 WS63 为主要支持目标，并为更多芯片预留扩展点

## 核心能力

- SEBOOT 协议通信（帧封装、ACK 处理）
- YMODEM 文件传输与 CRC16-XMODEM
- FWPKG 固件包解析
- 串口/端口抽象（native + wasm 实验支持）
- 设备发现与常见 USB 转串口芯片识别
- 芯片家族抽象（`ChipFamily` / `Flasher`）
- 可取消操作模型（`CancelContext`）

## 功能特性（Cargo Features）

- `native`（默认）：Linux / macOS / Windows 原生串口支持
- `wasm`：WASM / Web Serial（实验性）
- `serde`：为部分类型提供序列化支持

## 安装与依赖

在你的项目中添加：

或在同一 workspace 内通过 path 依赖：

- crates.io：`hisiflash`
- workspace path：`hisiflash = { path = "../hisiflash" }`

## 取消机制（CancelContext）

长耗时操作（如烧录、擦除）可通过 `CancelContext` 实现可中断：

- CLI 场景：可使用 `cancel_context_from_global()` 绑定全局中断标记
- 嵌入场景：可传入自定义 checker 闭包，按业务逻辑中断

## 模块结构（简要）

- `protocol`：SEBOOT、YMODEM、CRC
- `image`：FWPKG 解析
- `target`：芯片抽象与实现
- `port`：端口 trait 与平台实现
- `device`：设备与接口识别
- `host`：主机侧发现与辅助能力
- `monitor`：监控输出处理工具

## 开发

在仓库根目录执行：

- `cargo check --all-targets`
- `cargo test`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo fmt --all`

更多背景见仓库根文档：

- `docs/ARCHITECTURE.md`
- `docs/protocols/PROTOCOL.md`
- 根 `README.md`

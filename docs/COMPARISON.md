# 功能对比分析

本文档对比 esptool、espflash、fbb_burntool、ws63flash 四个项目的功能，用于指导 hisiflash 的设计。

## 0. fbb_burntool CLI 参数完整对比

> 本节基于 fbb_burntool 源代码（`code/main.cpp`）中的实际 CLI 参数定义，与 hisiflash 当前实现进行逐一比对。

### 0.1 用户关注参数对比（重点）

| fbb_burntool 参数 | 格式 | 含义 | hisiflash 等价 | 状态 |
|---|---|---|---|---|
| `-com:<n>` | 值参数 | COM 口编号（Windows），如 `-com:3` → COM3 | `-p/--port <path>` | ✅ 已实现（跨平台路径形式）|
| `-bin:<path>` | 值参数 | 固件包/bin 文件路径（FWPKG 或裸 bin）| `flash <firmware>` / `write --bin` | ✅ 已实现 |
| `-chiptype:<type>` | 值参数 | 目标芯片型号（如 WS63、BS2X、BS25）| `-c/--chip <chip>` | ✅ 已实现 |
| `-signalbaud:<rate>` | 值参数 | UART 波特率（等同于 `-baud`）| `-b/--baud <rate>` | ✅ 已实现 |
| `-erasemode:<n>` | 值参数 | 擦除模式：`0`=按固件包参数擦除（normal）；`1`=先全片擦除再烧写；`2`=不擦除（需 flash 已为空）| `flash`（默认 normal）/ `erase --all` | ⚠️ 部分实现（无独立 erasemode 参数，仅有 `erase --all`）|
| `-onlyeraseall` | 开关 | 仅全片擦除，不烧写任何文件 | `erase --all` | ✅ 已实现 |
| `-onlyburn:<filter>` | 值参数 | 仅烧写指定名称的分区/文件，逗号分隔 | `flash --filter <names>` | ✅ 已实现 |
| `-reset` | 开关 | 烧写完成后复位设备 | 默认行为（烧写后自动复位）| ✅ 已实现（默认开启，暂无禁用选项）|
| `-switchafterloader` | 开关 | 下载 Loader 后再切换波特率（兼容 Hi3863/Hi3516）| `flash --late-baud` | ✅ 已实现 |
| `-beforereset` | — | **源码中不存在此参数**，可能是文档误记或旧版本特性 | — | ❌ 原工具不存在 |
| `-packagesize:<n>` | 值参数 | YMODEM 每包数据大小（字节），可选值：1024/2048/4096/8192；3x 芯片可选 4096/20480 | — | ❌ 未实现（hisiflash 固定使用 1024）|

### 0.2 fbb_burntool 全量 CLI 参数清单

以下参数均来自 `code/main.cpp` 源代码，分组整理：

#### 连接参数

| 参数 | 格式 | 含义 |
|---|---|---|
| `-com:<n>` | 值 | COM 口编号（整数，如 `3` 表示 COM3）|
| `-baud:<rate>` | 值 | 波特率（与 `-signalbaud` 等效）|
| `-signalbaud:<rate>` | 值 | 波特率（与 `-baud` 等效）|
| `-ipaddr:<addr>` | 值 | TCP 连接 IP 地址 |
| `-ipport:<port>` | 值 | TCP 连接端口 |
| `-jlinkpath:<path>` | 值 | JLink 可执行文件路径 |
| `-sle` | 开关 | 使用 SLE（星闪）无线连接方式，切换芯片类型为 SLEBS2X |
| `-address:<addr>` | 值 | SLE 设备蓝牙地址 |
| `-addresstype:<type>` | 值 | SLE 地址类型 |

#### 固件/目标参数

| 参数 | 格式 | 含义 |
|---|---|---|
| `-chiptype:<type>` | 值 | 目标芯片类型 |
| `-bin:<path>` | 值 | 固件包路径（FWPKG/bin）|
| `-3x` | 开关 | 指定 SPARTA（3x 系列）芯片 |
| `-flashboot` | 开关 | 使用 Flash Loader 启动模式 |
| `-romboot` | 开关 | 使用 ROM 启动模式 |

#### 烧写控制参数

| 参数 | 格式 | 含义 |
|---|---|---|
| `-erasemode:<n>` | 值 | 擦除模式（0=normal/按包参数，1=先全擦，2=不擦）|
| `-onlyeraseall` | 开关 | 仅全片擦除，不烧写 |
| `-onlyburn:<name>` | 值 | 仅烧写指定名称的分区，支持逗号分隔多个 |
| `-reset` | 开关 | 烧写成功后复位设备 |
| `-switchafterloader` | 开关 | 下载 Loader 后切换波特率 |
| `-packagesize:<n>` | 值 | YMODEM 包大小（字节）：1024/2048/4096/8192 |
| `-2ms` | 开关 | 打断间隔 2ms（发送中断帧的时间间隔）|
| `-burninterval:<ms>` | 值 | 自定义打断间隔（毫秒），需在合法范围内 |
| `-timeout:<ms>` | 值 | 连接超时时间（毫秒）|
| `-forceread:<ms>` | 值 | 定时读取串口模式，以毫秒为间隔 |
| `-informal` | 开关 | 非正式（非工厂）烧写模式 |

#### DFU（USB）参数

| 参数 | 格式 | 含义 |
|---|---|---|
| `-dfu` | 开关 | 使用 USB DFU 模式（默认 BS25）|
| `-bs25dfu` | 开关 | 使用 BS25 USB DFU 模式 |
| `-bs21dfu` | 开关 | 使用 BS21 USB DFU 模式 |
| `-autodfu` | 开关 | 自动 DFU 模式（非 HID）|
| `-hiddfu` | 开关 | HID DFU 模式 |
| `-vid:<id>` | 值 | USB VID（十六进制或十进制）|
| `-pid:<id>` | 值 | USB PID（十六进制或十进制）|
| `-devicepathid:<id>` | 值 | USB 设备路径 ID |
| `-usblocation:<loc>` | 值 | USB 物理位置 |
| `-usage:<id>` | 值 | HID Usage ID |
| `-usagepage:<id>` | 值 | HID Usage Page |
| `-gethiddevice` | 开关 | 获取 HID 设备信息 |

#### eFuse 参数

| 参数 | 格式 | 含义 |
|---|---|---|
| `-readefuse` | 开关 | 读取 eFuse |
| `-startbit:<n>` | 值 | eFuse 起始位 |
| `-bitwidth:<n>` | 值 | eFuse 位宽 |

#### Flash 导出参数

| 参数 | 格式 | 含义 |
|---|---|---|
| `-export` | 开关 | 烧写后导出 Flash 内容 |
| `-target:<type>` | 值 | 导出目标类型 |
| `-addr:<address>` | 值 | 导出起始地址 |
| `-size:<size>` | 值 | 导出数据大小 |

#### 界面/日志参数

| 参数 | 格式 | 含义 |
|---|---|---|
| `-console` | 开关 | 附加到控制台输出（重定向 stdout/stdin）|
| `-show` | 开关 | 显示 GUI 界面 |
| `-clearlog` | 开关 | 只保留工具自身日志，清除其他 |
| `-server:<addr>` | 值 | 服务端地址（集成场景）|

### 0.3 hisiflash vs fbb_burntool 完整差距分析

| 功能点 | fbb_burntool 参数 | hisiflash 现状 | 优先级 |
|---|---|---|---|
| 端口指定 | `-com:<n>` | ✅ `-p/--port <path>` | — |
| 固件路径 | `-bin:<path>` | ✅ `flash <firmware>` | — |
| 芯片类型 | `-chiptype:<type>` | ✅ `-c/--chip` | — |
| 波特率 | `-signalbaud:<rate>` / `-baud:<rate>` | ✅ `-b/--baud` | — |
| 擦除模式选择 | `-erasemode:<0\|1\|2>` | ⚠️ 无统一选项，需 `-onlyeraseall` 替代 | P1 |
| 仅全擦 | `-onlyeraseall` | ✅ `erase --all` | — |
| 仅烧写指定分区 | `-onlyburn:<name>` | ✅ `flash --filter <names>` | — |
| 烧写后复位 | `-reset` | ✅ 默认行为 | — |
| Loader 后切速 | `-switchafterloader` | ✅ `flash --late-baud` | — |
| 包大小配置 | `-packagesize:<n>` | ❌ 固定 1024 字节 | P2 |
| 打断间隔配置 | `-burninterval:<ms>` / `-2ms` | ❌ 无 | P2 |
| 连接超时配置 | `-timeout:<ms>` | ❌ 无 | P2 |
| 定时读取串口 | `-forceread:<ms>` | ❌ 无 | P3 |
| TCP/IP 连接 | `-ipaddr` / `-ipport` | ❌ 规划 P2 | P2 |
| USB DFU | `-dfu` / `-bs25dfu` / `-bs21dfu` | ❌ 规划 P2 | P2 |
| JLink 调试器 | `-jlinkpath` | ❌ 规划 P3 | P3 |
| SLE 无线烧写 | `-sle` / `-address` | ❌ 规划 P3 | P3 |
| eFuse 读取 | `-readefuse` / `-startbit` / `-bitwidth` | ❌ 规划 P3 | P3 |
| Flash 导出 | `-export` / `-target` / `-addr` / `-size` | ❌ 未规划 | P3 |

> **注:** `-beforereset` 参数在 fbb_burntool 源码中不存在，可能是文档误记。`-erasemode` 整数值：`0`=normal（按包配置擦），`1`=先全擦再烧，`2`=完全不擦。

---

## 1. 项目基本信息对比

| 特性 | esptool | espflash | fbb_burntool | ws63flash | hisiflash (规划) |
|------|---------|----------|--------------|-----------|------------------|
| 语言 | Python | Rust | Qt/C++ | C (GNU) | Rust |
| 界面 | CLI | CLI + Lib | GUI | CLI | CLI + Lib |
| 平台 | 跨平台 | 跨平台 | Windows | 跨平台 | 跨平台 |
| 许可 | GPL-2.0 | MIT/Apache-2.0 | Proprietary | GPL-3.0 | MIT/Apache-2.0 |
| 维护方 | Espressif | esp-rs 社区 | HiSilicon | 社区 | 社区 |
| 目标芯片 | ESP 系列 | ESP 系列 | HiSilicon 全系 | WS63 | HiSilicon (优先 WS63) |

## 2. ws63flash 功能详解 (优先参考)

ws63flash 是通过逆向 BurnTool 实现的 WS63 烧写工具，功能简洁但完整：

### 2.1 核心命令

| 命令 | 描述 | 示例 |
|------|------|------|
| `--flash` | 烧写 fwpkg 固件包 | `ws63flash --flash /dev/ttyUSB0 firmware.fwpkg` |
| `--write` | 烧写裸机 bin 文件 | `ws63flash --write /dev/ttyUSB0 loader.bin app.bin@0x230000` |
| `--write-program` | 烧写并签名程序 | `ws63flash --write-program /dev/ttyUSB0 app.bin` |
| `--erase` | 擦除 Flash | `ws63flash --erase /dev/ttyUSB0 firmware.fwpkg` |

### 2.2 选项

| 选项 | 描述 |
|------|------|
| `-b BAUD` | 设置波特率 (推荐 921600) |
| `--late-baud` | LoaderBoot 后切换波特率 (Hi3863) |
| `-v` | 详细输出 |

### 2.3 协议特点

- 海思私有帧格式: `0xDEADBEEF + LEN + CMD + SCMD + DATA + CRC16`
- YMODEM 协议传输文件
- CRC16-XMODEM 校验
- 内置 LoaderBoot 签名二进制

## 3. 连接方式对比

| 连接方式 | esptool | espflash | fbb_burntool | ws63flash | hisiflash (规划) |
|----------|---------|----------|--------------|-----------|------------------|
| 串口 (UART) | ✅ | ✅ | ✅ | ✅ | ✅ P0 |
| USB CDC | ✅ | ✅ | ❌ | ❌ | ✅ P1 |
| USB JTAG | ✅ | ✅ | ❌ | ❌ | ❌ |
| USB DFU | ❌ | ❌ | ✅ | ❌ | ✅ P2 |
| TCP/IP | ✅ (RFC2217) | ❌ | ✅ | ❌ | ✅ P2 |
| JLink | ❌ | ❌ | ✅ | ❌ | ✅ P3 |
| WiFi (SLE) | ❌ | ❌ | ✅ | ❌ | ✅ P3 |

## 4. 核心命令对比

### 4.1 烧录相关

| 命令 | esptool | espflash | fbb_burntool | ws63flash | hisiflash |
|------|---------|----------|--------------|-----------|-----------|
| 烧写固件 | `write_flash` | `flash` | `-bin:<path>` CLI | `--flash` | `flash` |
| 读取Flash | `read_flash` | `read-flash` | `-export` CLI | ❌ | ❌（规划）|
| 擦除全部 | `erase_flash` | `erase-flash` | `-onlyeraseall` CLI | `--erase` | `erase --all` |
| 擦除区域 | `erase_region` | `erase-region` | ❌ | ❌ | ❌（规划）|
| 擦除模式控制 | ❌ | ❌ | `-erasemode:<0\|1\|2>` | ❌ | ❌（规划 P1）|
| 分区过滤烧写 | ❌ | ❌ | `-onlyburn:<name>` | ❌ | `flash --filter` |
| CRC 预检（跳过） | `verify_flash` | `--no-verify` | ❌ | ❌ | `flash --skip-verify` |
| 裸机烧写 | ❌ | ❌ | ❌ | `--write` | `write` |
| 程序烧写 | ❌ | ❌ | ❌ | `--write-program` | `write-program` |

### 4.2 信息查询

| 命令 | esptool | espflash | fbb_burntool | hisiflash |
|------|---------|----------|--------------|-----------|
| 固件包信息 | `image_info` | ❌ | ❌ | `info <firmware>` |
| 固件包信息（JSON）| ❌ | ❌ | ❌ | `info --json` |
| 设备信息 | `chip_id` | `board-info` | 显示在UI | ❌（规划）|
| Flash ID | `flash_id` | ❌ | ❌ | ❌（规划）|
| MAC地址 | `read_mac` | ❌ | ❌ | ❌（规划）|
| MD5/CRC校验 | 内置 | `checksum-md5` | ❌ | ❌（规划）|

### 4.3 镜像处理

| 命令 | esptool | espflash | fbb_burntool | hisiflash |
|------|---------|----------|--------------|-----------|
| ELF转镜像 | `elf2image` | 内置 | ❌ | ❌（规划）|
| 合并镜像 | `merge_bin` | `save-image` | 合并fwpkg | ❌（规划）|
| 分区表查看 | ❌ | `partition-table` | ❌ | ❌（规划）|

### 4.4 设备控制

| 命令 | esptool | espflash | fbb_burntool | hisiflash |
|------|---------|----------|--------------|-----------|
| 复位 | `--after hard_reset` | `reset` | `-reset` CLI | ❌（规划）烧写后自动复位 |
| 保持复位 | ❌ | `hold-in-reset` | ❌ | ❌（规划）|
| 加载到RAM | `load_ram` | ❌ | ❌ | ❌（规划）|
| 运行 | `run` | ❌ | ❌ | ❌（规划）|
| Loader后切速 | ❌ | ❌ | `-switchafterloader` | `flash --late-baud` |

### 4.5 调试功能

| 命令 | esptool | espflash | fbb_burntool | hisiflash |
|------|---------|----------|--------------|-----------|
| 串口监控 | ❌ | `monitor` | 简单日志 | `monitor` |
| 读内存 | `read_mem` | ❌ | ❌ | ❌（规划）|
| 写内存 | `write_mem` | ❌ | ❌ | ❌（规划）|
| 导出内存 | `dump_mem` | ❌ | ❌ | ❌（规划）|

### 4.6 安全功能

| 命令 | esptool (espefuse) | espflash | fbb_burntool | hisiflash |
|------|-------------------|----------|--------------|-----------|
| 读eFuse | `espefuse.py summary` | 内置 | `-readefuse -startbit -bitwidth` CLI | ❌（规划 P3）|
| 写eFuse | `espefuse.py burn_efuse` | ❌ | ✅ GUI | ❌（规划 P3）|
| 安全信息 | `get_security_info` | ❌ | ❌ | ❌（规划）|
| 签名 | `espsecure.py` | ❌ | ❌ | ❌（规划）|
| 加密 | `espsecure.py` | ❌ | ✅ | ❌（规划）|

## 5. 固件格式支持对比

| 格式 | esptool | espflash | fbb_burntool | hisiflash |
|------|---------|----------|--------------|-----------|
| ELF | ✅ | ✅ | ❌ | ✅ P1 |
| BIN | ✅ | ✅ | ✅ | ✅ P0 |
| Intel HEX | ✅ | ❌ | ✅ | ✅ P1 |
| ESP Image | ✅ | ✅ | ❌ | ❌ |
| FWPKG | ❌ | ❌ | ✅ | ✅ P0 |
| UF2 | ✅ | ❌ | ❌ | ❌ |

## 6. 配置文件对比

### esptool
- 环境变量: `ESPTOOL_CHIP`, `ESPTOOL_PORT`, `ESPTOOL_BAUD` 等
- 命令行参数为主，无配置文件

### espflash
- `espflash.toml` - 项目配置
- `espflash_ports.toml` - 串口配置
- 支持全局配置和项目配置

### fbb_burntool
- INI 格式配置文件
- `configure/` 目录下的配置
- GUI 设置保存

### hisiflash (规划)
- `hisiflash.toml` - 项目配置
- `hisiflash_ports.toml` - 串口配置
- 全局配置: `~/.config/hisiflash/config.toml`
- 环境变量支持

## 7. 芯片/目标支持对比

### esptool 支持的芯片
- ESP8266
- ESP32, ESP32-S2, ESP32-S3, ESP32-S31
- ESP32-C2, ESP32-C3, ESP32-C5, ESP32-C6, ESP32-C61
- ESP32-H2, ESP32-H21, ESP32-H4
- ESP32-P4

### espflash 支持的芯片
- ESP32
- ESP32-C2, ESP32-C3, ESP32-C5, ESP32-C6
- ESP32-H2
- ESP32-P4
- ESP32-S2, ESP32-S3

### fbb_burntool 支持的芯片
- WIFI5GNB
- LUOFU (0x30005)
- XILING (0x30006)
- EMEI (0x30007)
- TG0, TG1, TG2
- MCU 系列
- SLE 系列

### hisiflash (规划)
- Phase 1: WIFI5GNB, 基础芯片
- Phase 2: LUOFU, XILING, EMEI
- Phase 3: TG 系列, MCU
- Phase 4: SLE 及其他扩展

## 8. 架构特点对比

### esptool 架构特点
- **优点:**
  - 功能最完整
  - 文档完善
  - 社区活跃
  - 支持 Stub Loader 加速
- **缺点:**
  - Python 依赖
  - 启动较慢
  - 分发不便

### espflash 架构特点
- **优点:**
  - Rust 实现，性能好
  - 可作为库使用
  - 单文件分发
  - 现代化 CLI
- **缺点:**
  - 功能不如 esptool 完整
  - 不支持 eFuse 写入
  - 社区较小

### fbb_burntool 架构特点
- **优点:**
  - GUI 友好
  - 支持多种连接方式
  - 海思芯片原生支持
- **缺点:**
  - 仅 Windows
  - 无法脚本自动化
  - 闭源

### hisiflash (规划) 设计目标
- 结合 esptool 的功能完整性
- 采用 espflash 的 Rust 架构
- 支持 fbb_burntool 的连接方式
- 跨平台 + CLI + 库

## 9. 人体工程学对比 (Ergonomics)

### 9.1 CLI 交互体验

| 特性 | esptool | espflash | ws63flash | hisiflash (规划) |
|------|---------|----------|-----------|------------------|
| **进度显示** | 文本百分比 `[=>  ] 75%` | 彩色进度条 | 原地更新百分比 | 彩色进度条 ✅ |
| **颜色输出** | 基础 ANSI | ✅ 丰富 (crossterm) | ❌ | ✅ 丰富 |
| **表格输出** | ❌ | ✅ (comfy-table) | 简单 ASCII | ❌ |
| **输出折叠** | ✅ Stage 折叠 | ❌ | ❌ | ✅ P2 |
| **详细模式** | `-v` | `RUST_LOG=debug` | `-v/-vv/-vvv` | `-v/-vv/-vvv` ✅ |
| **静默模式** | `-q` | ❌ | ❌ | `-q/--quiet` ✅ |

### 9.2 交互式功能

| 特性 | esptool | espflash | ws63flash | hisiflash (规划) |
|------|---------|----------|-----------|------------------|
| **交互式串口选择** | ❌ | ✅ (dialoguer) | ❌ | ✅ P1 |
| **已知设备高亮** | ❌ | ✅ 粗体显示 | ❌ | ✅ P1 |
| **确认提示** | ❌ | ✅ 记住串口/确认端口 | ❌ | ✅ P1 |
| **非交互模式** | 默认 | `--non-interactive` | 默认 | `--non-interactive` |
| **Ctrl-C 处理** | 基础 | ✅ 光标恢复 | 基础 | ✅ 优雅退出 |

### 9.3 自动检测与智能功能

| 特性 | esptool | espflash | ws63flash | hisiflash (规划) |
|------|---------|----------|-----------|------------------|
| **芯片自动检测** | ✅ Magic 值 | ✅ | ❌ 需指定 | ✅ 从文件名推断 |
| **Flash 大小检测** | ✅ JEDEC ID | ✅ | ❌ | ❌ P2 |
| **USB VID/PID 过滤** | ✅ `--port-filter` | ✅ 内置已知设备 | ❌ | ✅ 海思设备优先 |
| **串口自动排序** | ✅ Espressif 优先 | ✅ 已知设备优先 | ❌ | ✅ 已知设备优先 |
| **macOS tty 过滤** | ✅ | ✅ 过滤 /dev/tty.* | ❌ | ❌ P2 |
| **Windows COM 转换** | ❌ | ❌ | ✅ COM→/dev/ttyS | ❌（原生路径格式）|

### 9.4 错误处理与提示

| 特性 | esptool | espflash | ws63flash | hisiflash (规划) |
|------|---------|----------|-----------|------------------|
| **错误美化** | 基础 | ✅ miette fancy | 基础 perror | ✅ anyhow+thiserror |
| **故障排除链接** | ✅ 链接到文档 | ❌ | ❌ | ✅ P2 |
| **Linux 权限提示** | ✅ dialout 提示 | ❌ | ❌ | ✅ |
| **建议修复操作** | ❌ | ❌ | ❌ | ✅ P2 |
| **安全检查警告** | ✅ Secure Boot | ❌ | ❌ | ✅ P3 |

### 9.5 配置与环境变量

| 特性 | esptool | espflash | ws63flash | hisiflash (规划) |
|------|---------|----------|-----------|------------------|
| **环境变量** | ✅ 完整 | ✅ 基础 | ❌ | ✅ 完整 |
| **配置文件** | ✅ esptool.cfg | ✅ TOML | ❌ | ✅ TOML |
| **本地配置** | ✅ 当前目录 | ✅ 工作区 | ❌ | ✅ |
| **全局配置** | ✅ ~/.config | ✅ ~/.config | ❌ | ✅ |
| **串口记忆** | ❌ | ✅ 保存到配置 | ❌ | ✅ P1 |
| **项目配置分离** | ❌ | ✅ espflash_ports.toml | ❌ | ✅ |

**esptool 环境变量:**
```bash
ESPTOOL_CHIP, ESPTOOL_PORT, ESPTOOL_BAUD, ESPTOOL_BEFORE, ESPTOOL_AFTER
ESPTOOL_FF, ESPTOOL_FM, ESPTOOL_FS, ESPTOOL_CONNECT_ATTEMPTS
```

**espflash 环境变量:**
```bash
ESPFLASH_PORT, ESPFLASH_BAUD, MONITOR_BAUD, ESPFLASH_SKIP_UPDATE_CHECK
```

**hisiflash 环境变量 (已实现):**
```bash
HISIFLASH_PORT, HISIFLASH_BAUD, HISIFLASH_CHIP, HISIFLASH_LANG, HISIFLASH_NON_INTERACTIVE
```

### 9.6 Shell 补全与 CLI 辅助

| 特性 | esptool | espflash | ws63flash | hisiflash (规划) |
|------|---------|----------|-----------|------------------|
| **Shell 补全** | ❌ | ✅ Bash/Zsh/Fish/PowerShell | ❌ | ✅ completions 子命令 |
| **串口 Tab 补全** | ❌ | ❌ | ❌ | ❌ P2 |
| **波特率补全** | ✅ | ❌ | ✅ 列出可用 | ❌ |
| **芯片名补全** | ❌ | ❌ | N/A | ✅（ValueEnum 自动）|
| **@ 文件参数** | ✅ `@args.txt` | ❌ | ❌ | ✅ P3 |

### 9.7 数值解析

| 特性 | esptool | espflash | ws63flash | hisiflash (规划) |
|------|---------|----------|-----------|------------------|
| **十六进制** | ✅ 0x | ✅ 0x/0X | ✅ 0x | ✅ |
| **下划线分隔** | ❌ | ✅ `0x12_34` | ❌ | ✅ |
| **大小后缀** | ✅ k/M | ✅ k/M | ❌ | ❌ P2 |
| **all 关键字** | ❌ | ✅ `--size all` | ❌ | ❌ P2 |

### 9.8 烧写后操作

| 特性 | esptool | espflash | ws63flash | hisiflash (规划) |
|------|---------|----------|-----------|------------------|
| **烧写后监控** | ❌ | ✅ `-M/--monitor` | ❌ | ✅ `flash --monitor` |
| **烧写后验证** | ✅ 内置 | ✅ `--no-verify` 禁用 | ❌ | ⚠️ `--skip-verify` 禁用 CRC 预检（非读回验证）|
| **校验和跳过** | ❌ | ✅ 匹配则跳过 | ❌ | ❌ P2 |
| **自动复位** | ✅ `--after` | ✅ 默认 | ✅ | ✅ 烧写后自动复位 |
| **保持 stub** | ✅ `--after no_reset_stub` | ❌ | N/A | N/A |

### 9.9 串口监控功能

| 特性 | esptool | espflash | ws63flash | hisiflash (规划) |
|------|---------|----------|-----------|------------------|
| **独立 monitor 命令** | ❌ | ✅ | ❌ | ✅ P1 |
| **快捷键复位** | N/A | ✅ Ctrl+R | N/A | ✅ |
| **defmt 支持** | N/A | ✅ | N/A | ✅ P3 |
| **地址解析** | N/A | ✅ ELF 符号 | N/A | ✅ P3 |
| **外部处理器** | N/A | ✅ `--processors` | N/A | ✅ P3 |

### 9.10 重试与超时机制

| 特性 | esptool | espflash | ws63flash | hisiflash (规划) |
|------|---------|----------|-----------|------------------|
| **连接重试** | ✅ 7次默认 | ✅ | ❌ | ✅ 7次（硬编码）|
| **可配置重试次数** | ✅ 环境变量 | ❌ | ❌ | ❌ P2 |
| **写块重试** | ✅ 3次 | ❌ | ❌ | ✅ 3次（硬编码）|
| **动态超时** | ✅ 按大小计算 | ❌ | 固定超时 | ❌ P2 |
| **自定义复位序列** | ✅ 配置文件 | ❌ | ❌ | ✅ P3 |

**esptool 超时配置:**
```ini
[esptool]
timeout = 3
chip_erase_timeout = 120
md5_timeout_per_mb = 8
erase_region_timeout_per_mb = 30
connect_attempts = 7
```

### 9.11 更新与版本

| 特性 | esptool | espflash | ws63flash | hisiflash (规划) |
|------|---------|----------|-----------|------------------|
| **自动更新检查** | ❌ | ✅ update-informer | ❌ | ✅ P3 |
| **跳过更新检查** | N/A | ✅ `-S` | N/A | ✅ |
| **版本信息** | ✅ `--version` | ✅ | ✅ | ✅ |
| **构建信息** | ❌ | ❌ | ✅ git 版本 | ✅ git hash |

### 9.12 特殊 CLI 特性

| 特性 | esptool | espflash | ws63flash | hisiflash (规划) |
|------|---------|----------|-----------|------------------|
| **命令分组** | ✅ Basic/Advanced | ❌ | ❌ | ✅ |
| **选项分组** | ✅ rich_click | ❌ | ❌ | ✅ |
| **下划线→连字符** | ✅ 带警告 | ❌ | ❌ | ✅ |
| **命令别名** | ✅ 旧命令兼容 | ❌ | ❌ | ❌ |
| **互斥选项检查** | ✅ | ❌ | ❌ | ✅ clap |

## 10. 功能对比汇总

### 10.1 基础用户体验

| 特性 | esptool | espflash | ws63flash | hisiflash |
|------|---------|----------|-----------|-----------|
| 进度显示 | 文本百分比 | 彩色进度条 | 百分比 | 彩色进度条 |
| 颜色输出 | 基础 | ✅ 丰富 | ❌ | ✅ 丰富 |
| 错误提示 | 基础+链接 | 友好 | 基础 | 友好（anyhow）|
| 自动检测芯片 | ✅ | ✅ | ❌ | ✅（文件名推断）|
| 自动检测串口 | ✅ | ✅ | ❌ | ✅ |
| Shell补全 | ❌ | ✅ | ❌ | ✅ completions |
| 配置文件 | ✅ | ✅ | ❌ | ✅ TOML |
| 环境变量 | ✅ 完整 | ✅ 基础 | ❌ | ✅ PORT/BAUD/CHIP/LANG/NON_INTERACTIVE |

## 11. 值得借鉴的设计

### 从 esptool 借鉴
1. **Stub Loader 机制** - 上传小程序到 RAM 加速烧写
2. **完整的命令集** - 功能全面
3. **环境变量支持** - 便于 CI/CD
4. **详细的错误信息** - 便于调试
5. **配置文件超时** - 可配置各种超时参数
6. **故障排除链接** - 错误信息带文档链接
7. **命令分组显示** - Basic/Advanced 分组
8. **动态超时计算** - 根据数据大小调整
9. **自定义复位序列** - 配置文件支持
10. **@ 文件参数扩展** - 从文件读取参数

### 从 espflash 借鉴
1. **Trait-based 架构** - 易于扩展
2. **Feature flags** - 按需编译
3. **库/CLI 分离** - 可复用
4. **现代化 CLI** - 用户友好
5. **配置文件分离** - 项目/端口分开
6. **交互式串口选择** - dialoguer 库
7. **已知设备高亮** - 粗体显示
8. **串口记忆功能** - 保存到配置文件
9. **烧写后监控** - `-M` 选项
10. **校验和跳过** - 内容匹配则跳过烧写
11. **miette 错误美化** - fancy 错误输出
12. **自动更新检查** - update-informer

### 从 ws63flash 借鉴
1. **分级详细模式** - `-v/-vv/-vvv`
2. **分区表可视化** - ASCII 表格显示
3. **内置 loaderboot** - 无需额外文件
4. **Late baud 模式** - 兼容 Hi3863
5. **地址语法** - `file@0x230000`
6. **Windows COM 转换** - 自动处理

### 从 fbb_burntool 借鉴
1. **多连接方式** - Serial（`-com`）/ TCP（`-ipaddr/-ipport`）/ USB DFU（`-dfu`）/ JLink（`-jlinkpath`）/ SLE（`-sle`）
2. **FWPKG 格式** - 海思固件包（`-bin`）
3. **加密支持** - AES 加密
4. **状态机设计** - 烧写流程控制
5. **擦除模式参数化** - `-erasemode` 分三级（normal/全擦/不擦）
6. **YMODEM 包大小配置** - `-packagesize` 可调 1024/2048/4096/8192 字节
7. **打断间隔配置** - `-burninterval/-2ms` 适配不同硬件
8. **连接超时配置** - `-timeout` 毫秒级控制
9. **Flash 导出功能** - `-export/-target/-addr/-size`

## 12. hisiflash 差异化特性 (规划)

1. **多连接统一接口** - 不同连接方式使用相同的上层 API
2. **插件式芯片支持** - 新芯片可通过配置文件添加
3. **智能重试机制** - 自动处理常见错误
4. **并行烧写支持** - 多设备同时烧写 (future)
5. **远程烧写** - 支持网络远程烧写
6. **完整的测试覆盖** - 模拟测试 + 硬件测试

## 13. 遗漏功能清单 (需补充)

以下是对比分析中发现的遗漏功能，应在 hisiflash 中考虑实现：

### 12.1 高优先级 (P0-P1) ✅ 已完成

| 功能 | 来源 | 说明 | 状态 |
|------|------|------|------|
| **交互式串口选择** | espflash | 多串口时自动提示选择 | ✅ 已实现 |
| **串口记忆** | espflash | 记住上次使用的串口 | ✅ 已实现 |
| **环境变量完整支持** | esptool | PORT/BAUD/CHIP/NON_INTERACTIVE | ✅ 已实现 |
| **烧写后监控** | espflash | `-M` 自动进入监控模式 | ✅ 已实现 |
| **分级详细模式** | ws63flash | `-v/-vv/-vvv` 三级调试 | ✅ 已实现 |
| **Shell 补全生成** | espflash | `completions` 子命令 | ✅ 已实现 |
| **非交互模式** | espflash | `--non-interactive` | ✅ 已实现 |
| **USB VID/PID 扩展** | esptool | 支持 CH340/CP210x/FTDI/PL2303/Espressif | ✅ 已实现 |
| **配置文件支持** | espflash | TOML 配置 (local + global) | ✅ 已实现 |
| **静默模式** | espflash | `-q/--quiet` | ✅ 已实现 |
| **端口确认** | espflash | `--confirm-port` | ✅ 已实现 |

### 12.2 中优先级 (P2)

| 功能 | 来源 | 说明 |
|------|------|------|
| **校验和跳过** | espflash | 内容匹配则跳过烧写 |
| **Flash 大小检测** | esptool | JEDEC ID 检测 |
| **故障排除链接** | esptool | 错误信息带文档链接 |
| **串口 Tab 补全** | hisiflash | 动态补全可用串口 |
| **输出折叠** | esptool | 隐藏中间输出，只显示结果 |
| **动态超时** | esptool | 根据数据大小计算超时 |
| **擦除模式参数** | fbb_burntool | `-erasemode:<0\|1\|2>` 三级：normal/先全擦/不擦 |
| **YMODEM 包大小配置** | fbb_burntool | `-packagesize:<n>`，支持 1024/2048/4096/8192 |
| **打断间隔配置** | fbb_burntool | `-burninterval:<ms>` 适配不同硬件时序 |
| **连接超时配置** | fbb_burntool | `-timeout:<ms>` 毫秒级连接等待 |
| **TCP/IP 远程连接** | fbb_burntool | `-ipaddr/-ipport` 网络烧写 |
| **USB DFU 模式** | fbb_burntool | `-dfu/-bs25dfu/-bs21dfu` USB 升级 |

### 12.3 低优先级 (P3)

| 功能 | 来源 | 说明 |
|------|------|------|
| **自动更新检查** | espflash | 检查新版本并提示 |
| **自定义复位序列** | esptool | 配置文件定义复位序列 |
| **@ 文件参数** | esptool | 从文件读取命令行参数 |
| **defmt 支持** | espflash | 解析 defmt 日志 |
| **ELF 符号解析** | espflash | 监控时解析地址为函数名 |
| **外部日志处理器** | espflash | 管道到外部程序 |
| **JLink 调试器接入** | fbb_burntool | `-jlinkpath` 通过 JLink 烧写 |
| **SLE 无线烧写** | fbb_burntool | `-sle/-address` 星闪无线协议 |
| **eFuse CLI 读写** | fbb_burntool | `-readefuse/-startbit/-bitwidth` |
| **Flash 导出** | fbb_burntool | `-export/-target/-addr/-size` 读出 Flash 内容 |
| **定时读取模式** | fbb_burntool | `-forceread:<ms>` 定时轮询串口 |

## 14. 人体工程学设计原则

基于对比分析，hisiflash 应遵循以下设计原则：

### 13.1 零配置开箱即用
- 自动检测串口和芯片
- 合理的默认值
- USB VID/PID 自动识别海思设备

### 13.2 渐进式复杂度
- 简单命令立即可用: `hisiflash flash firmware.fwpkg`
- 高级选项按需添加: `hisiflash flash -b 921600 -v firmware.fwpkg`
- 配置文件持久化常用设置

### 13.3 清晰的反馈
- 彩色进度条显示进度
- 操作成功/失败有明确提示
- 错误信息包含解决建议

### 13.4 可脚本化
- 非交互模式支持 CI/CD
- 环境变量覆盖所有关键参数
- 退出码规范 (0=成功, 1=运行时错误, 2=用法错误, 3=配置错误, 130=Ctrl-C/中断)

### 13.5 可调试
- 分级详细模式 (-v/-vv/-vvv)
- 协议帧十六进制输出
- 超时和重试可配置

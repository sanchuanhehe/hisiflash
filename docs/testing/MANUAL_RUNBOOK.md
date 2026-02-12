# 手工联测操作手册（Manual Runbook）

> 适用场景：真实设备联测、发布前冒烟、问题复现。
>
> 目标：给出可直接执行的命令与预期结果，配合 `MANUAL_CHECKLIST.md` 打勾验收。

## 0. 前置条件

- 主机：Linux/macOS/Windows（建议 Linux）
- 设备：已连接串口（示例使用 `/dev/ttyUSB0`）
- 固件：可用 `.fwpkg`（示例：`firmware.fwpkg`）
- 可执行文件：`hisiflash`（或使用 `cargo run --bin hisiflash -- ...`）

建议先设置变量：

```bash
export PORT=/dev/ttyUSB0
export FW=firmware.fwpkg
```

---

## 1. 基础连通性

### 1.1 查看帮助与命令列表

```bash
hisiflash --help
```

预期：
- 正常输出帮助
- 包含 `flash / erase / monitor / list-ports / info`

### 1.2 枚举串口

```bash
hisiflash list-ports
```

预期：
- 能看到目标串口（如 `$PORT`）
- 自动识别信息正常（若驱动/VID PID 支持）

---

## 2. 完整烧录主链路

### 2.1 执行烧录

```bash
hisiflash flash -p "$PORT" "$FW"
```

预期：
- 烧录过程完成到 100%
- 结束提示正常（完成/复位）
- 进程退出码为 `0`

可用以下方式确认退出码：

```bash
echo $?
```

---

## 3. 烧录后 monitor 联动

### 3.1 烧录后自动进入 monitor

```bash
hisiflash flash -p "$PORT" "$FW" --monitor --monitor-baud 115200
```

预期：
- 烧录成功后自动进入 monitor
- 能持续接收串口输出
- `Ctrl+C` 可正常退出

---

## 4. monitor 交互专项（重点）

### 4.1 基础 monitor

```bash
hisiflash monitor -p "$PORT" --monitor-baud 115200
```

预期：
- 显示 monitor 打开提示
- 持续输出串口日志

### 4.2 Ctrl+T 时间戳切换

操作：在 monitor 中按 `Ctrl+T` 两次。

预期：
- 第一次提示“时间戳已启用”
- 第二次提示“时间戳已禁用”

### 4.3 Ctrl+R 复位证据分级

操作：在 monitor 中按 `Ctrl+R`。

预期输出顺序（语义）：
1. `正在重启设备 (DTR/RTS 切换)...`
2. `复位信号已发送。`
3. 证据分级之一：
   - `已观察到复位证据（启动特征输出）。`
   - `已观察到静默后新输出...设备可能已复位。`
   - `未观察到明确复位证据。`

注意：
- 当出现明确证据时，不应再出现“流控连线提示”。
- “流控连线提示”仅应出现在弱证据/未确认/复位发送失败场景。

### 4.4 启动特征样例验证（建议）

当设备输出类似以下内容，应命中“已观察到复位证据”：

```text
boot.
Flash Init Fail! ret = 0x80001341
verify_public_rootkey secure verify disable!
verify_params_key_area secure verify disable!
```

### 4.5 clean/raw 模式

默认 clean：

```bash
hisiflash monitor -p "$PORT" --clean-output
```

原始 raw：

```bash
hisiflash monitor -p "$PORT" --raw
```

预期：
- `--clean-output` 过滤不可打印控制字符
- `--raw` 原样输出串口数据

---

## 5. 中断语义（退出码 130）

### 5.1 flash 中断

```bash
hisiflash flash -p "$PORT" "$FW"
# 执行过程中按 Ctrl+C
```

预期：
- 进程退出码 `130`

### 5.2 erase 中断

```bash
hisiflash erase -p "$PORT" --all
# 执行过程中按 Ctrl+C
```

预期：
- 进程退出码 `130`

### 5.3 monitor 中断

```bash
hisiflash monitor -p "$PORT"
# 执行过程中按 Ctrl+C
```

预期：
- 进程退出码 `130`
- 再次启动 monitor/flash 不应出现端口残留占用

---

## 6. non-TTY 分流契约

### 6.1 stdout/stderr 分流检查

```bash
hisiflash monitor -p "$PORT" --monitor-baud 115200 \
  > monitor.stdout.log 2> monitor.stderr.log
```

预期：
- 串口正文主要在 `monitor.stdout.log`
- 状态提示（打开/重置/关闭）在 `monitor.stderr.log`

---

## 7. 常见问题排查

### 7.1 `Ctrl+R` 无复位效果

- 检查板卡是否支持 DTR/RTS 复位连线
- 检查 USB 转串口芯片驱动与权限
- 缩短串口线，避免接触不良

### 7.2 端口权限问题（Linux）

```bash
sudo usermod -a -G dialout $USER
# 重新登录后生效
```

### 7.3 设备占用

- 关闭其他串口工具（minicom/screen/串口助手）
- 确认同一时刻只有一个进程占用串口

---

## 8. 结果记录模板

```text
日期：
测试人：
设备：
端口：
固件：
commit：

通过项：
失败项：
关键日志：
结论：通过 / 有风险 / 不通过
```

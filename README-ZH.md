<p align="center">
  <img src="res/logo-header.svg" alt="RustDesk - 您的远程桌面"><br>
  <a href="#构建">构建</a> •
  <a href="#安装">安装</a> •
  <a href="#许可证">许可证</a> •
  <a href="#截图">截图</a> •
  <a href="#常见问题">常见问题</a> •
  <a href="#已知问题">已知问题</a>
</p>

# SpacemiT K1 支持

这个分支为 **SpacemiT K1** RISC-V 开发板添加了支持，允许您使用 RustDesk 远程控制 K1 设备。

## 功能特性

- **远程桌面**：访问远程设备的桌面
- **远程终端**：访问远程设备的终端
- **剪贴板同步**：与远程开发板共享剪切板
- **远程音频**：播放远程设备的声音
- **自定义服务器配置**：支持通过命令行设置Rustdesk Server
- **视频硬件加速**：支持 H.264/H.265 硬件编码
- **P2P直连**：支持打洞直连，延迟更低

## 构建

### 前置要求

- 硬件：[SpacemiT K1](https://www.spacemit.com/key-stone-k1/)
- 操作系统：[Bianbu Desktop 3.0 或更高版本](https://bianbu.spacemit.com/)

### 安装依赖

```bash
sudo apt update
sudo apt install rustup sccache clang mold \ 
libvpx-dev libyuv-dev libaom-dev libopus-dev\ 
libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
libgtk-3-dev \
libavutil-dev libavcodec-dev libavformat-dev libavfilter-dev libswscale-dev libavdevice-dev \
libpam0g-dev libxdo-dev libxcb-randr0-dev
rustup default nightly
```

创建 `/usr/lib/pkgconfig/libyuv.pc`, 内容如下：

```bash
prefix=/usr
exec_prefix=${prefix}
libdir=${exec_prefix}/lib/riscv64-linux-gnu
includedir=${prefix}/include

Name: libyuv
Description: YUV library
Version: 0.0.1904
Libs: -L${libdir} -lyuv
Cflags: -I${includedir}/libyuv
```

### 克隆代码

```bash
git clone --recurse-submodules https://github.com/qianqiu-2020/rustdesk.git
cd rustdesk
```

### 构建调试版本

```bash
./build_simple_deb.sh
```

### 构建发布版本

```bash
./build_simple_deb.sh -m release
```

### 为其他平台构建

请使用 GitHub 工作流为其他平台构建：[Actions](https://github.com/qianqiu-2020/rustdesk/actions)

## 安装

### 架构

```bash
客户端 -> 服务器 -> 设备
```

客户端：Windows/Linux/Mac/Android/Web 客户端

服务器：RustDesk 官方服务器 / 自建服务器

设备：SpacemiT K1 开发板

### 在客户端上安装

#### Windows 客户端

**下载**：

[下载自定义客户端](https://github.com/qianqiu-2020/rustdesk/releases/download/1.4.2/rustdesk-1.4.2-x86_64.exe)，该版本添加了对 K1 设备 H.264/H.265 流解码的支持。

**配置**：

1. 配置服务器

- 默认使用 RustDesk 官方服务器
- 自建服务器，请参考 [在服务器上安装](#在服务器上安装)，然后按照下图将服务器配置字符串导入客户端：
![设置服务器配置](set_server_config.png)

2. 启用 UDP 打洞以提高连接成功率
  ![启用 UDP 打洞](enable_udp_hole_punching.png)

3. 如果使用自建服务器，请导出服务器配置字符串供设备导入
![导出配置](export_config.png)

#### Web 客户端

- 如果您使用官方服务器，请参考 [RustDesk Web 客户端](https://rustdesk.com/web/)

- 如果您使用自建服务器，请访问 <http://ip:21114/webclient2/>，将 `ip` 替换为您服务器的公网 IP 地址。

#### 其他客户端

- 自定义版本（支持解码来自 K1 的 H.264/H.265 流）：[GitHub Releases](https://github.com/qianqiu-2020/rustdesk/releases)
- 官方版本：[GitHub Releases](https://github.com/rustdesk/rustdesk/releases)

### 在设备上安装

**要求**：Bianbu Desktop、Bianbu Desktop Lite 3.0 或更高版本

#### 安装 RustDesk

- Bianbu Desktop

```bash
sudo apt update
sudo apt install rustdesk
```

- Bianbu Desktop Lite

```bash
sudo apt update
sudo apt install xdg-desktop-portal-wlr libxdo3 lxqt-wayland-session 
systemctl --user start xdg-desktop-portal-wlr
systemctl --user restart xdg-desktop-portal
sudo apt install rustdesk
```

> **注意：** 在 Bianbu Desktop Lite 上安装后，光标会暂时变为十字形。将其移动到您想要共享的屏幕中并左键单击。

#### 配置 RustDesk 服务器

默认使用 RustDesk 官方服务器，如果您想使用自己的服务器，请导入从 Windows 客户端导出的服务器配置字符串：

```bash
sudo rm -r ~/.config/rustdesk /root/.config/rustdesk
rustdesk --import-config ==XXX
sudo systemctl restart rustdesk
```

#### 获取设备 ID 和密码

- 设备 ID：

```bash
rustdesk --get-id                    
```

- 获取临时密码：

```bash
journalctl -u rustdesk -g 'Temporary Password:'
```

- 设置永久密码：

```bash
sudo rustdesk --password bianbu
```

### 在服务器上安装

如果您想使用自己的服务器，可以参考 [rustdesk-api 文档](https://github.com/lejianwen/rustdesk-api) 进行部署。

以下是使用 [Docker Compose](https://docs.docker.com/engine/install/) 部署自建 RustDesk 服务器的示例：

```bash
mkdir -p /data/rustdesk/api /data/rustdesk/server
mkdir -p ~/rustdesk-server
touch ~/rustdesk-server/docker-compose.yml
```

docker-compose.yml 内容：

```yaml
networks:
  rustdesk-net:
    external: false

services:
  rustdesk-server:
    container_name: rustdesk-server
    ports:
      - 21114:21114
      - 21115:21115
      - 21116:21116
      - 21116:21116/udp
      - 21117:21117
      - 21118:21118
      - 21119:21119
    image: harbor.spacemit.com/application/rustdesk-server-s6:latest
    environment:
      - RELAY=<ip>:21117
      - ENCRYPTED_ONLY=1
      - MUST_LOGIN=Y
      - TZ=Asia/Shanghai
      - RUSTDESK_API_RUSTDESK_ID_SERVER=<ip>:21116
      - RUSTDESK_API_RUSTDESK_RELAY_SERVER=<ip>:21117
      - RUSTDESK_API_RUSTDESK_API_SERVER=http://<ip>:21114
      - RUSTDESK_API_APP_SHOW_SWAGGER=1
    volumes:
      - /data/rustdesk/api:/app/data
      - /data/rustdesk/server:/data
    networks:
      - rustdesk-net
    restart: unless-stopped
```

将 `<ip>` 替换为您服务器的公网 IP 地址。

启动服务器：

```bash
docker compose up -d
```

获取管理员账户初始密码：

```bash
docker compose logs | grep -E "(Key:|Admin Password)"
```

## 许可证

本项目使用 GNU Affero General Public License v3.0 许可证 - 详情请参阅 [LICENCE](LICENCE) 文件。

## 截图

![连接到 musepi](connecting_to_musepi.png)

## 常见问题

### Web 客户端无法访问剪贴板

**原因**：Chrome 认为 HTTP 源不安全，因此阻止了剪贴板访问。

**解决方案**：

```text
1. 访问 chrome://flags/#unsafely-treat-insecure-origin-as-secure
2. 将 http://<rustdesk_serverip>:21114 添加到列表中
3. 重启浏览器并刷新 WebClient 页面
4. 在弹出的权限对话框中允许剪贴板访问
```

## 已知问题

目前，Sciter 和 Flutter（RustDesk 使用的 UI 框架）都不支持 riscv64 平台。因此，SpacemiT K1 设备上只能使用 RustDesk 的 CLI 版本，与完整的 GUI 版本相比功能有限。例如，您无法通过图形界面修改设置或从 K1 设备发起连接到其他设备。
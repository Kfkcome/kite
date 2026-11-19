<p align="center">
  <img src="packaging/macos/icon.svg" width="128" alt="Kite icon: a crisp white diamond kite on near-black">
</p>

<h1 align="center">Kite</h1>

<p align="center">
  基于 Rust 与 <a href="https://github.com/zed-industries/zed/tree/main/crates/gpui">GPUI</a> 构建的原生 SSH 客户端。<br>
  Termius / Netcatty 的高性能、高颜值、低内存占用开源替代方案。
</p>

---

## 为什么选择 Kite

市面上主流的 SSH 客户端（如 Termius、Netcatty、Tabby）大多基于 Electron / Web 技术栈，打开几个终端标签往往就要占用数百 MB 内存。

**Kite** 使用纯 **Rust** 编写，底层采用 **GPUI** GPU 加速渲染、**russh** 原生异步 SSH 协议栈与 **alacritty_terminal** 终端仿真内核：

- **极低内存占用与极致性能**：单原生进程运行，内存占用仅为 Electron 客户端的零头（单连接会话约 ~70 MB），滚动、分屏与高吞吐输出丝滑流畅。
- **高颜值原生界面**：内置多款名画壁纸与自适应莫奈取色系统，支持明暗主题自动跟随系统、自定义等宽字体、亚克力/半透明背景效果。
- **开箱即用的效率工具**：
  - `⌘K` 快速连接、分组与标签管理、`~/.ssh/config` 一键导入
  - 支持 **SOCKS5 / HTTP CONNECT / ProxyCommand / ProxyJump** 代理连接
  - 支持自由拖拽分屏（`⌘D` / `⌘⇧D`）、标签页拆窗与滚动条自动隐藏/拖拽
  - 内置 **SFTP** 侧边栏文件浏览、下载与拖拽上传
  - **Snippets** 快捷命令片段（`⇧⌘S`）、端口转发（Local / Remote / SOCKS）
  - **age 加密保险箱**：PIN / Keychain 解锁、外部密码管理器联动、`⌘\` 一键填充 `sudo` 密码
  - 支持加密同步至个人私有 Git 仓库，无需账号与订阅

## 构建与运行

需要 Rust 工具链（见 [docs/building.md](docs/building.md)）：

```sh
# 运行开发版
cargo run -p kite

# 构建 macOS .app 应用包（输出至 target/release/bundle/Kite.app）
./scripts/bundle-app.sh
```

## 致谢

- 本项目基于开源项目 **tern** 持续演进与二次开发
- 感谢 **linux.do** 社区及所有反馈与支持的朋友

## 开源协议

本项目采用 **[GNU General Public License v3.0 or later (GPL-3.0-or-later)](LICENSE)** 开源协议。

第三方依赖与衍生代码声明见 [NOTICE.md](NOTICE.md)。

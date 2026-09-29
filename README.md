# 密码本 (Password Notebook) — Rust + 原生 Win32

本地优先的密码管理工具。**直接用 Win32/ComCtl32 原生控件**,不含 WebView、不含任何运行时依赖。

最终产物是**单个 528 KB 的 exe**,拷到任何 Windows 10/11 上双击即可运行。

---

## 为什么是这个体积

| 方案 | 体积 |
|---|---|
| **本项目(Rust + Win32)** | **528 KB,免安装** |
| C# WPF 依赖框架版 | ~25 MB(其中 Windows Hello 的 WinRT 投影占 24.8 MB) |
| C# WPF 单文件自包含 | ~68 MB |

体积构成:标准库 + `windows` crate 绑定 + `aes-gcm` / `argon2` / `serde_json`。
运行时全部来自 Windows 自带系统 DLL(`user32` / `comctl32` / `comdlg32` / `crypt32` / `wtsapi32` 等)。

## 文件格式与路径

### `.pkk` 是本项目自创的格式

底层密码学都是公开标准(**Argon2id** = RFC 9106,**AES-256-GCM** = NIST SP 800-38D),
但**文件容器与扩展名是自创的**,不存在外部规范:

```
[魔数 "PNBK" 4B][文件头 105B][主密码槽 48B][恢复码槽 48B][载荷长度 4B][AES-256-GCM 载荷]
```

实际后果:

- 没有第三方工具能打开,包括其他密码管理器
- Windows 对它没有文件关联(双击不会打开本程序)
- 换扩展名不影响功能,真正的识别依据是文件头里的魔数 `PNBK`
- 将来要互通,现实做法是 CSV / JSON 导入导出,而不是去读别人的加密库

### 路径规则(全部由 exe 位置推导,无硬编码)

| 文件 | 位置 |
|---|---|
| 数据库 | exe 同目录,**固定文件名 `data.pkk`** |
| 免密缓存 | `%LOCALAPPDATA%\PasswordNotebook\quickunlock.dat` |

**磁盘上只有一个数据文件。** 设置(剪贴板秒数、空闲锁定、回收站保留天数、免密开关等)
**加密存放在数据库内部**,不再有独立的 `settings.json`。

**只有一个数据库,文件名固定**,不做多库管理:

- 启动时只判断 `data.pkk` 在不在 —— **在就进入解锁界面,不在就走初始化创建流程**
- 因此不会出现「多个数据库文件不知道该用哪个」的问题
- 路径全部来自 `std::env::current_exe()` 的父目录,没有任何写死的绝对路径
- exe 所在目录**不可写**时(例如放进 Program Files),自动回退到「我的文档」
- 免密缓存必须是 `%LOCALAPPDATA%`:它用 DPAPI 绑定到本机账户,跟着 U 盘走没有意义

## 功能

- 条目管理:标题、用户名、密码、网址、分类、标签、备注
- 搜索(标题/用户名/网址/标签/备注)、按分类与标签筛选、三种排序
- 密码生成器(长度、字符集、排除易混淆字符)
- 剪贴板保护:复制密码后按设定秒数自动清空
- 回收站:可恢复,超期自动彻底删除
- 自动锁定:锁屏(Win+L)/ 屏保激活立即锁定,也可自定义空闲超时
- 修改主密码、用恢复码找回主密码
- 本机免密解锁(DPAPI)+ 可选 Windows Hello(指纹/人脸/PIN)验证
- 单实例:重复启动会把已有窗口拉到前台

## 安全设计

保险库是单个可移植文件 `.pnb`:

```
[文件头 105B][主密码槽 48B][恢复码槽 48B][载荷长度 4B][AES-256-GCM 载荷]
```

- 随机生成 32 字节**数据密钥(DEK)**加密载荷;**AES-256-GCM**,每次保存换新随机数
- DEK 被两个独立密钥槽包裹,**Argon2id**(默认 64 MiB / t=3 / p=4):
  - 主密码槽:`KEK = Argon2id(主密码)`
  - 恢复码槽:`KEK = Argon2id(恢复码)`
- **三套 AAD 相互独立**(主密码槽 / 恢复码槽 / 载荷):
  改主密码只需重包一个槽,不碰恢复码槽与载荷;反之亦然
- 文件头是明文但不含秘密,并作为 AAD 参与认证 —— 任何篡改都会导致解密失败

### 本机免密解锁

- 解锁后用 **Windows DPAPI(CurrentUser)** 包裹 DEK 写入 `%LOCALAPPDATA%\PasswordNotebook\quickunlock.dat`
- 缓存与 `vault_id + key_generation` 绑定,**改主密码后旧缓存自动失效**
- 检测到**锁屏 / 屏保**立即清除缓存,需要重新输入主密码
- 可要求免密解锁前先通过 **Windows Hello** 验证

> 边界:程序未运行时发生的锁屏无法感知。要加严就勾选「免密解锁时要求 Windows Hello 验证」,或关闭免密解锁。

## 目录结构

```
src/
├─ main.rs                入口(仅 Windows)
├─ lib.rs
├─ crypto.rs              AES-256-GCM 与 Argon2id 封装
├─ header.rs              文件头 + 三套 AAD 构造
├─ vaultfile.rs           文件读写(原子写入)
├─ model.rs               Entry / Document / Settings
├─ recovery.rs            恢复码(Base32)
├─ generator.rs           密码生成器
├─ strength.rs            强度评估
├─ paths.rs               路径与设置读写
├─ vault.rs               保险库服务(解锁/保存/改密/条目 CRUD)
└─ win/                   Windows 原生层(仅 Windows 编译)
   ├─ mod.rs              入口、单实例、DPI、通用控件初始化
   ├─ sys.rs              Win32 常量
   ├─ ui.rs               控件与窗口封装(隐藏新类型)
   ├─ app.rs              全局状态
   ├─ dialog.rs           模态对话框框架
   ├─ main_window.rs      顶层窗口与确定性退出路径
   ├─ main_ui.rs          四种形态:锁定 / 创建 / 已解锁 / 回收站
   ├─ dlg_editor.rs       条目编辑
   ├─ dlg_settings.rs     设置 + 修改主密码 + 重新生成恢复码
   ├─ dlg_generator.rs    密码生成器
   ├─ dlg_recovery.rs     恢复码展示 / 用恢复码找回
   ├─ dpapi.rs            本机免密缓存
   ├─ hello.rs            Windows Hello
   ├─ clipboard.rs        剪贴板
   ├─ idle.rs             空闲与屏保
   └─ timefmt.rs          时间格式化
```

## 构建

前置条件(WSL 内一次性):

```bash
sudo apt-get install -y gcc-mingw-w64-x86-64
rustup target add x86_64-pc-windows-gnu
```

```bash
./build.sh                 # 产出 target/x86_64-pc-windows-gnu/release/PasswordNotebook.exe
cargo test                 # 纯逻辑测试(在 Linux 上原生跑,不需要 Windows)
```

开发用小工具(生成一个测试密码本,顺便验证格式跨平台一致):

```bash
cargo run --example make_vault -- ./data.pkk '测试主密码'
```

自检(会创建窗口并自动关闭,用于验证退出路径,并打印解析出的全部路径与扫描结果):

```bash
PasswordNotebook.exe --selftest
```

## 关于「退出后进程不消失」

退出路径是刻意设计成确定性的:唯一的顶层窗口销毁 → `WM_DESTROY` → `PostQuitMessage`
→ 消息循环退出 → `run()` 返回 → 进程结束。没有任何后台线程持有进程,
`--selftest` 里的「窗口生命周期」一项就是专门验收这条的。

## 已知限制

- 暂无导入 / 导出(CSV / JSON)
- 程序未运行期间发生的锁屏无法感知(见上文「边界」)
- KDF 参数在创建密码本时固定
- 界面文字为简体中文,未做多语言

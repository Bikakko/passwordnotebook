# PasswordNotebook — Rust + 原生 Win32

本地优先的密码本。**直接用 Win32/ComCtl32 原生控件**,不含 WebView、不含任何运行时依赖。
最终产物是**单个约 600 KB 的 `pnb.exe`(免安装)**,拷到任何 Windows 10/11 上双击即可运行,
图标等资源全部内嵌在 exe 里。

## 为什么是这个体积

| 方案 | 体积 |
|---|---|
| **本项目(Rust + Win32)** | **约 600 KB,免安装** |
| C# WPF 依赖框架版 | ~25 MB(其中 Windows Hello 的 WinRT 投影占 24.8 MB) |
| C# WPF 单文件自包含 | ~68 MB |

体积构成:标准库 + `windows` crate 绑定 + `aes-gcm` / `argon2` / `serde_json`,加上内嵌图标。
运行时全部来自 Windows 自带系统 DLL(`user32` / `comctl32` / `comdlg32` / `crypt32` / `wtsapi32` 等)。

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

## 文件格式与路径

### `.pkk` 是本项目自创的格式

底层密码学都是公开标准(**Argon2id** = RFC 9106,**AES-256-GCM** = NIST SP 800-38D),
但容器与扩展名是自创的,不存在外部规范:

```
[文件头 105B][主密码槽 48B][恢复码槽 48B][载荷长度 4B][AES-256-GCM 载荷]
```

文件头是明文、不含秘密,魔数 `PNBK` **就在这 105 字节里**,不是额外前缀:

```
魔数 "PNBK" 4 + 版本 1 + m 4 + t 4 + p 4 + vault_id 16 + key_generation 4
+ 主密码盐 16 + 主密码随机数 12 + 恢复码盐 16 + 恢复码随机数 12 + 载荷随机数 12 = 105
```

因此:没有第三方工具能打开它,Windows 也没有文件关联(双击不会打开本程序);换扩展名不影响
功能,识别依据始终是文件头里的 `PNBK`。将来要互通,现实做法是 CSV / JSON 导入导出,
而不是去读别人的加密库。

### 路径规则(全部由 exe 位置推导,无硬编码)

| 文件 | 位置 |
|---|---|
| 数据库 | exe 同目录,**固定文件名 `data.pkk`** |
| 免密缓存 | `%LOCALAPPDATA%\PasswordNotebook\quickunlock.dat` |

**磁盘上只有一个数据文件**:设置(剪贴板秒数、空闲锁定、回收站保留天数、免密开关等)
加密存在数据库内部,没有独立的 `settings.json`。也**只有一个数据库、文件名固定** ——
启动时只判断 `data.pkk` 在不在,在就进解锁界面、不在就走初始化创建,所以不会出现
「多个库不知道该用哪个」。exe 所在目录不可写时(例如放进 Program Files)自动回退到「我的文档」。
免密缓存必须放 `%LOCALAPPDATA%`:它用 DPAPI 绑定到本机账户,跟着 U 盘走没有意义。

## 安全设计

- 随机生成 32 字节**数据密钥(DEK)**加密载荷;**AES-256-GCM**,每次保存换新随机数
- DEK 被两个独立密钥槽包裹,**Argon2id**(默认 64 MiB / t=3 / p=4):
  主密码槽 `KEK = Argon2id(主密码)`,恢复码槽 `KEK = Argon2id(恢复码)`
- **三套 AAD 相互独立**(主密码槽 / 恢复码槽 / 载荷):改主密码只重包一个槽,不碰另外两个
- 文件头明文但不含秘密,并作为 AAD 参与认证 —— 任何篡改都会导致解密失败

### 本机免密解锁

- 解锁后用 **Windows DPAPI(CurrentUser)** 包裹 DEK 写入上面那个缓存文件
- 缓存与 `vault_id + key_generation` 绑定,**改本程序登录密码后旧缓存自动失效**
- 检测到**锁屏 / 屏保**立即清除缓存;可要求先通过 **Windows Hello** 验证

> 边界:程序未运行期间发生的锁屏无法感知。要加严就勾选「免密解锁时要求 Windows Hello 验证」,
> 或直接关掉免密解锁。
>
> 别误判两点:缓存绑的是**本机这个 Windows 账户**,不是 Windows 密码 ——
> **改自己的 Windows 登录密码并不会让它失效**(要踢掉请用程序里的「锁定」,或在设置里关掉免密解锁);
> 反过来,同一 Windows 账户下的其它进程也能解密它。

## 目录结构

```
src/
├─ main.rs / lib.rs        入口
├─ crypto.rs               AES-256-GCM 与 Argon2id 封装
├─ header.rs               文件头 + 三套 AAD 构造
├─ vaultfile.rs            文件读写(原子写入)
├─ vault.rs                保险库服务(解锁/保存/改密/条目 CRUD)
├─ model.rs                Entry / Document / Settings
├─ recovery.rs             恢复码(Base32)
├─ generator.rs            密码生成器
├─ strength.rs             强度评估
├─ paths.rs                路径推导与设置读写
├─ error.rs                错误类型
└─ win/                    Windows 原生层(仅 Windows 编译)
   ├─ mod.rs               入口、单实例、DPI、通用控件初始化
   ├─ sys.rs               Win32 常量
   ├─ ui.rs                控件与窗口封装(隐藏新类型)
   ├─ app.rs               全局状态
   ├─ dialog.rs            模态对话框框架
   ├─ main_window.rs       顶层窗口与确定性退出路径
   ├─ main_ui.rs           四种形态:锁定 / 创建 / 已解锁 / 回收站
   ├─ window_state.rs      窗口位置状态(存注册表)
   ├─ dlg_editor.rs        条目编辑
   ├─ dlg_input.rs         单行输入对话框
   ├─ dlg_settings.rs      设置 + 修改主密码 + 重新生成恢复码
   ├─ dlg_generator.rs     密码生成器
   ├─ dlg_recovery.rs      恢复码展示 / 用恢复码找回
   ├─ dlg_taxonomy.rs      分类与标签管理
   ├─ dpapi.rs             本机免密缓存
   ├─ hello.rs             Windows Hello
   ├─ clipboard.rs         剪贴板
   ├─ idle.rs              空闲与屏保
   ├─ timefmt.rs           时间格式化
   └─ selftest.rs          自检与界面预览
```

## 构建

前置条件(WSL 内一次性):

```bash
sudo apt-get install -y gcc-mingw-w64-x86-64
rustup target add x86_64-pc-windows-gnu
```

```bash
./build.sh                 # 产出 target/x86_64-pc-windows-gnu/release/pnb.exe
cargo test                 # 纯逻辑测试(在 Linux 上原生跑,不需要 Windows)
pnb.exe --selftest         # 自检:建窗口、跑核心逻辑、打印全部路径,然后自动退出
cargo run --example make_vault -- ./data.pkk '测试主密码'   # 生成一个测试库
```

退出路径是刻意设计成确定性的:唯一的顶层窗口销毁 → `WM_DESTROY` → `PostQuitMessage`
→ 消息循环退出 → `run()` 返回 → 进程结束,没有后台线程持有进程。
`--selftest` 里的「窗口生命周期」一项就是专门验收这条的。

## 已知限制

- 暂无导入 / 导出(CSV / JSON)
- 程序未运行期间发生的锁屏无法感知(见上文「边界」)
- KDF 参数在创建密码本时固定
- 界面文字为简体中文,未做多语言
- **内存里的敏感数据只能做到「尽量短命」,做不到彻底消除**。DEK、派生密钥、解密后的整份库
  明文、条目密码、剪贴板与消息框的宽字符缓冲,丢弃时都会清零(`Zeroizing`);但有两类抹不掉:
  Win32 输入框自己持有的那份文本,以及 `serde_json` 解析 / 序列化过程中的中间副本。
  所以能读本进程内存的攻击者(内存取证、崩溃转储、休眠文件)仍可能捞到明文 ——
  清零的作用是抬高门槛、缩短窗口,不是消除风险。

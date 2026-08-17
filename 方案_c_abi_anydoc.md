# 方案：路线 C —— 通过 cbindgen 给 anydoc 加 C ABI，C++ 侧 dlopen 原生库

> 目标：不重写、不依赖 Node 运行时，让纯 C++ 项目以「进程内动态库」方式复用 anydoc 的全部转换能力。
> 方式：在 anydoc 之上增加一个 `extern "C"` 导出层，用 `cbindgen` 生成 C 头文件，产出 `.dylib/.so/.dll`，C++ 侧 `dlopen` 加载调用。
>
> 本文档聚焦「方案 + 难度评估」，是立项前的技术评估稿。

---

## 0. 结论先行

1. **技术可行，且是三条路线里「去依赖、性能、可控性」最优解**：C++ 直接链原生库，无 Node、无进程启动开销、可静态掌控生命周期。
2. **唯一代价**：需要在 anydoc 仓库里**新增一个 ~200~400 行的 C 导出 shim**（`capi/` 模块），并**引入 Rust 工具链做一次性编译**。
3. **真正的设计难点只有一个**：anydoc 的 `Document` 是深度嵌套、递归、多态的数据结构，**无法直接跨 C 边界**。必须做「关键决策」（见 §3），这是本方案 90% 的复杂度来源。
4. **整体难度：中低**。核心 Rust 逻辑零改动，只是加一层序列化/内存管理胶水。预估 **2~4 周**（1 名工程师，含三平台构建验证）。

---

## 1. 现状盘点（anydoc 侧）

| 检查项 | 现状 | 对方案的影响 |
| --- | --- | --- |
| 已有 C ABI 导出 | **无**（0 个 `extern "C"` / `#[no_mangle]` / `repr(C)`） | 需从零新建 |
| cbindgen 配置 | **无** | 需新建 `cbindgen.toml` |
| 核心入口 | `to_markdown(path)` / `to_markdown_bytes(&[u8], fmt)` / `to_document(&[u8], fmt)` / `Format::from_bytes/from_extension/from_path` | 已足够，接口简洁 |
| 错误类型 | `ConvertError`（6 变体），已有 `code()` 返回稳定字符串 | 好：可直接映射成 C 的 `int` 枚举 + 消息字符串 |
| 核心数据模型 | `Document { blocks, notes, assets }`，含递归 enum（`Block`/`Inline`/`Table`/`List`/`Style`…） | **跨 FFI 的难点所在**（见 §3） |
| 跨平台构建 | 已是纯 Rust crate，`cargo build --release` 即可出 `.dylib/.so/.dll` | 无需特殊处理 |

---

## 2. 总体架构

```
┌─────────────────────────── anydoc (Rust) ───────────────────────────┐
│  src/lib.rs          现有核心：14 格式解析 + 统一模型 + GFM 序列化    │
│                                                                     │
│  src/capi.rs   【新增】extern "C" 导出层（shim）                     │
│     ├─ anydoc_to_markdown(path)          → C 字符串 + 错误码          │
│     ├─ anydoc_to_markdown_bytes(data,len,fmt) → 同上                 │
│     ├─ anydoc_to_document_bytes(data,len,fmt) → 序列化文档(JSON/C)    │
│     ├─ anydoc_format_from_bytes(...) / from_extension / from_path    │
│     └─ anydoc_string_free(...) / anydoc_last_error(...)              │
│                                                                     │
│  cbindgen.toml    【新增】→ 生成 include/anydoc.h                     │
└─────────────────────────────────────────────────────────────────────┘
                        │  cargo build --release
                        ▼
              libanydoc_capi.dylib / .so / .dll
                        │
┌────────────────────── C++ 集成方 ───────────────────────────────────┐
│  AnyDocBackend.h/.cpp  【新增】                                      │
│     ├─ dlopen/LoadLibrary 动态加载 libanydoc_capi                     │
│     ├─ 封装成 DocMarkdown::toMarkdown(path) 门面                     │
│     └─ 错误码 → 现有错误体系映射                                     │
└─────────────────────────────────────────────────────────────────────┘
```

---

## 3. 关键设计决策（本方案的核心难度）

### 3.1 【决策 1】富文档模型如何跨 C 边界？（最关键）

`Document` 是递归多态结构：`Block::List(List)` 里又含 `Vec<Block>`，`Inline::Link{content: Vec<Inline>}` 递归嵌套，`Table` 是二维 `Vec<Vec<CellSlot>>` 且单元格再嵌套 `Block`。**这种结构无法用 `repr(C)` 直接平铺**。

三个选项：

| 选项 | 做法 | 优点 | 缺点 | 难度 |
| --- | --- | --- | --- | --- |
| **A. 只导 Markdown（推荐）** | C ABI 只暴露 `to_markdown`，返回 UTF-8 字符串；不暴露 `Document` 结构 | 极简、无序列化开销、规避所有嵌套问题 | 拿不到结构化文档/资源 | ⭐ |
| **B. Document → JSON** | Rust 侧把 `Document` 序列化成 JSON 字符串返回（`serde_json`），C++ 侧用 `imgui_json` 解析 | 结构完整、两侧都有现成 JSON 库 | 序列化开销、丢失类型安全、C++ 需重写遍历 | ⭐⭐ |
| **C. 手写扁平 C 结构体** | 用 `cbindgen` 把 `Document` 逐层映射成 `repr(C)` 结构体 + 指针数组 | 零拷贝、类型安全 | **工作量巨大**（递归 enum 映射极繁琐，约 500~1000 行） | ⭐⭐⭐⭐⭐ |

**推荐：A（一期）+ B（可选二期）**。
- 文档「预览 / 解析」场景，**最终要的是 Markdown 文本**，选项 A 完全够用；
- 若后续需要内嵌图片/结构化数据，再上 B（serde_json 序列化，成本低）。

> 结论：**只要选 A，路线 C 的难度立刻从「中高」降到「低」**。这是本方案性价比的关键。

### 3.2 【决策 2】内存所有权模型

C ABI 跨边界传递堆内存，必须明确谁释放：

| 方案 | 做法 | 推荐度 |
| --- | --- | --- |
| **Caller-free（推荐）** | Rust 侧 `CString::into_raw` 返回裸指针，C++ 侧用完调 `anydoc_string_free(ptr)` | ✅ 简单、无悬挂 |
| Caller-allocated buffer | C++ 预分配缓冲区，Rust 填入 | ❌ 无法预知长度，需二次调用 |

推荐 **Caller-free**，配套导出 `anydoc_string_free`。

### 3.3 【决策 3】错误传递

- `ConvertError` 已有稳定 `code()`（`unsupported`/`malformed`/`encrypted`/`resourceLimit`/`missingPart`/`io`）。
- C ABI 层设计：返回 `int32_t` 错误码（0=成功，非 0=对应 `code` 的枚举值），`Display` 消息另通过 `anydoc_last_error()` 取字符串（可选）。
- 若用「返回指针 + 错误码」约定，则成功返回非空字符串，失败返回 NULL 并置错误码。

### 3.4 【决策 4】编译产物与分发

- 三个平台各出动态库：`libanydoc_capi.dylib`（mac）、`libanydoc_capi.so`（linux）、`anydoc_capi.dll`（win）。
- C++ 侧 `dlopen` / `LoadLibrary` 加载，**缺失时优雅降级**（回退到「无 Markdown 预览」或 CLI）。
- 可选：把 `.dylib` 打进 app bundle / 与可执行文件同目录分发。

---

## 4. 分阶段方案与进度

| 阶段 | 交付内容 | 核心工作 | 预估工作量 |
| --- | --- | --- | --- |
| **P0 Rust C 导出层** | `src/capi.rs` + `cbindgen.toml` + 生成 `anydoc.h` | 导出 3~5 个 `extern "C"` 函数（`to_markdown` + `format_from_*` + `string_free`）；决策 1 选 A；错误码映射 | 1~2 天 |
| **P1 三平台编译** | 出 `.dylib/.so/.dll` | `cargo build --release`；验证导出符号；mac/linux/win 各测一次 | 1~2 天 |
| **P2 C++ 加载层** | `AnyDocBackend.h/.cpp` | `dlopen`/`LoadLibrary` + 函数指针解析 + 门面 `toMarkdown(path)` + 错误映射 | 2~3 天 |
| **P3 集成进 app** | 文档预览走 Markdown 通道 | 接入现有预览流程；无库时降级提示；多页/资源处理 | 2~3 天 |
| **P4 打包与分发** | 动态库随 app 分发 | CMake 集成、bundle/目录布局、版本对齐 | 1~2 天 |
| **P5（可选）Document→JSON** | 决策 1 的 B 选项 | serde 序列化 + C++ 侧 JSON 解析，支持内嵌图片/结构化 | 1~2 周（可选） |

**累计：约 2~4 周（P0~P4 核心路径），P5 可选另计。**

---

## 5. 难度评估总结

| 风险/难点 | 等级 | 说明 | 对策 |
| --- | --- | --- | --- |
| 富模型跨 FFI | ⭐⭐⭐⭐⭐ | 唯一真正的难点 | **选决策 A（只导 Markdown）直接规避** |
| Rust 工具链引入 | ⭐⭐ | 团队需装 Rust 做一次性编译 | 编译产物可预编译进仓库，后续无需 Rust |
| 内存所有权 | ⭐⭐ | 跨边界堆内存 | Caller-free + `anydoc_string_free` |
| 三平台符号/加载差异 | ⭐⭐ | dlopen vs LoadLibrary、符号修饰 | C ABI 天然无 name mangling，标准做法 |
| 错误传递 | ⭐ | `code()` 已稳定 | 映射成 int 枚举 |
| cbindgen 配置 | ⭐ | 仅 3~5 个函数，纯标量/指针 | 极简 |
| 动态库分发 | ⭐⭐ | 路径查找、缺失降级 | dlopen 失败 → 降级提示 |

**总体：难度中低（2~4 周）。** 前提是接受「只导 Markdown」（决策 3.1 选项 A）。

---

## 6. 待确认的前置问题（立项前需拍板）

1. **是否接受「只拿 Markdown 字符串、不要结构化 Document」？** 这是把难度从「高」降到「低」的分水岭。
2. **是否接受在 anydoc 仓库里新增 `capi` 模块 + 引入 Rust 工具链编译？**（anydoc 是第三方仓库，要么 fork/子模块维护，要么上游贡献）
3. **动态库分发形态**：随 app bundle 打包，还是跟随系统安装路径？
4. **anydoc 版本锁定策略**：锁定某个 tag，升级时重新生成头文件 + 重编译。

---

## 7. 下一步行动（确认后即可启动）

1. 拍板 §6 四个前置问题；
2. 若确认，先做 **P0**：在 anydoc 里落地 `src/capi.rs`（只导 `to_markdown` + `format_from_*` + `string_free`），用 cbindgen 出 `anydoc.h`，本地 `cargo build --release` 验证 `.dylib` 导出符号；
3. 写一个最小 C++ demo 验证 `dlopen` + 调用 + 释放全链路，作为「难度探针」——这一步能在一两天内把路线 C 的最大不确定性（跨边界）彻底验掉。

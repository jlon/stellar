# Stellar 设计系统（MASTER）

> 来源：`ui-ux-pro-max`（Operate 模式）+ shadcn/ui（语义色/组件原生档位/克制配色）+ impeccable（quieter/operate/distill：扁平化、尺寸纪律、视觉降噪）+ 项目硬约束（ngx-admin/Nebular 原生优先）。
> 新页面/改版先读本文件；单页特例记 `docs/design-system/pages/<页>.md`，特例优先。
> 高级审美的具体执行标准见「高级审美落地细则」（§1 末尾），改任何 UI 前必读。

## 0. 硬约束（不可违背）

1. 前端以 `/mnt/data/ngx-admin` 为视觉和组件基线，优先复用其原生 Nebular、Bootstrap grid 和 `angular2-smart-table` 模式；仅在原生组件无法表达真实业务信息时做最小、兼容的自定义。组件用 Nebular 原生（nb-card/button/input/select/dialog/spinner/badge/tooltip/accordion/tabset/popover）。
2. 颜色/字号/圆角/间距用 Nebular 主题变量（`nb-theme(...)` / CSS var），禁止硬编码色值。
3. 自定义样式必须与原生风格兼容；reshaping 现有页时保持原有信息架构。

## 1. 布局（Operate：扫描性优先）

- 筛选区：一行紧凑排列，搜索框收窄（14~20rem），下拉固定宽（8~11rem），主动作按钮右对齐（`margin-left: auto`）。Nebular 的 `input[nbInput]` 用 `fieldSize="small"` / `fieldSize="tiny"` 控制尺寸；原生 `size` 属性无效。
- 集群运行时工具、会话、参数配置与权限、资源组同属“集群运维”分组；父项无直达路由，子项保持独立路由和权限。父分组不新增授权门槛，且无任一可见子项时必须隐藏，避免菜单出现空壳。
- 高级筛选默认收起，展开为扁平面板（无嵌套卡片），字段内联标签同行；其开关是次级工具动作，使用 `nbButton ghost size="small"` + `funnel-outline`，带 `title`、`aria-label`、`aria-expanded`，禁止使用高饱和 filled 状态按钮。
- 表格：列宽收敛，操作列图标化（hover 出现），空态给下一步指引（去诊断/重试/新建）。
- 回答/正文列宽 ≤46rem；代码块压高 24rem + 块内滚动。

### 响应式基线

- **不缩放页面、不滚动工具栏**：窄屏时正文、卡片和筛选/操作工具栏按语义分层，不通过整体缩小、文字逐字换行或横向滚动来“容纳”控件。只有二维数据区（smart-table、宽图表、代码/DDL）可在自身容器横向滚动。
- **按内容区而非 viewport 响应**：侧栏、页面 padding 和卡片内边距会显著缩小真实可用宽度。复杂工具栏优先用 `flex-wrap: wrap` + `margin-left: auto` 自动换行且保持贴右；仅当控件多到“换行后仍不可读”时才引入 `container-type: inline-size` 分档。
- **全局与局部职责分离**：`_overrides.scss` 只定义 Nebular / smart-table 等跨页不变量，不能按页面内部类名（如 `.header-filters-content`）补布局。页面自己在组件 SCSS 内定义工具栏布局。
- **重排顺序**：可用宽度不足时先让标题/上下文、筛选、批量写操作、刷新控制依次分行；文本控件保留完整可读宽度。危险操作不可因窄屏静默隐藏或压成难辨图标。中档禁止直接堆成又高又挤的四行长条。
- 桌面 smart-table 使用稳定的 `table-layout: fixed`，但操作列必须保留至少 5rem，禁止图标重叠或压成单字；窄屏（≤768px）改回自然列宽，在卡片容器内横向滚动，保留表头与行的完整可读性。表格溢出不得扩展到页面或影响侧边栏。
- 弹窗打开、下拉面板、toast 通知、表格首屏行入场动效已在全站落地，iOS 风格（短时长、低位移、仅 opacity/transform）。表格行错峰仅前 8 行，其余直接显示。页面切换入场在 `one-column.layout.scss`（选择器必须 `::ng-deep .main-column__body > :not(router-outlet)`，投影内容不带组件 scoping 属性）。所有入场动效被全局 `prefers-reduced-motion: reduce` 兜底禁用。
- - 页面为工作区临时紧凑全局侧栏时，必须保存并在离开该路由时恢复原始 `expanded` / `collapsed` 状态；启用路由复用后不能仅依赖组件的 `ngOnDestroy`。

### 极简工具栏（图标化）规范

源自会话管理页的实战迭代，全站页面头/卡片头工具栏统一执行：

1. **图标优先，零文字**：工具栏动作按钮一律 `nbButton ghost size="small" status="basic"` + 单个 Eva 图标；不写文字标签。语义由 `nbTooltip` + `title`（双保险）承载，可访问性加 `aria-label`（图标开关加 `aria-pressed`）。
2. **状态色彩化**：开关型控件激活态用主题色（`color-primary-default`）点亮，非激活中性 `text-hint-color`；禁止 filled 高饱和、禁止用红/黄按钮色表达“危险/警告”类别（写操作已有确认弹窗兜底）。
3. **统一间距与视觉重量**：同级图标间距全局统一（0.25rem），组与组之间不设额外 gap、分隔线或颜色差异；所有按钮同 size、同 shape。
4. **右对齐**：标题/上下文居左，控件组 `margin-left: auto` 贴右；窄屏换行后控件组仍贴右（auto margin 对换行后的单元素行同样生效）。
5. **冗余即删除**：低价值控件直接删除并清理死代码（例：会话页“手动刷新/自动刷新周期选择器”整体移除，保留刷新按钮）；“清除筛选”类功能通过再次点击开关完成，不设独立按钮。
6. **图标名必须先查证**：用 `require('eva-icons')` 的 key 集合或项目已在用清单确认存在（`moon`、`clock` 存在，`timer`、`hourglass` 不存在）。
7. **数据区均匀**：表格列宽均匀分配，无独宽列；长文本列截断 + tooltip；排序箭头占位计入列宽，避免表头截断（如 `Ses...`）。

### 高级审美自查清单（改版前过一遍）

1. 少即是多：每个像素都在传达信息吗？文字能换成图标吗？控件能删吗？
2. 一致性：同类控件同 size/appearance/status/间距吗？
3. 克制的颜色：颜色只在表达状态时出现，不做装饰吗？
4. 层级清晰：一眼能分清“哪里是标题、哪里是筛选、哪里是数据”吗？
5. 交互闭环：开关可再点取消、hover 有提示、危险操作有确认、loading/error/empty 三态齐全吗？
6. 不留死代码：删掉的控件，其样式、状态字段、方法、import 都清干净了吗？

### 高级审美落地细则（实战定稿）

源自多轮页面改版实测（菜单栏、Tab 栏、Header 工具栏、权限管理、筛选控件等），结合 shadcn/ui 与 impeccable（quieter/operate/distill）技能标准提炼。以上清单是“检查项”，本节是“具体怎么做”。

#### A. 扁平化（Flatness）

1. **禁卡中卡**：一页一个主内容卡；统计/筛选/操作并入主卡头部或主体，绝不另起卡片。结构层级用间距与分隔线表达，不用嵌套容器。
2. **工具栏扁平**：图标按钮一律 ghost/basic，无底色无描边无阴影；inactive 态与背景同层（操作面板“消失在任务里”）。
3. **减少 layering**：阴影、渐变、描边、圆角全部能省则省；分隔优先用 1px `divider-color` 细线而非边框盒子。

#### B. 尺寸纪律（Measure）

1. **图标与文字同量级**：工具栏图标 font-size 与正文一致（0.9375rem），禁止以组件高度为图标的默认值（Nebular `nb-action` 默认 `actions-small-icon-height` = 24px，必须覆盖）。
2. **Nebular 尺寸陷阱**：`nb-action[icon]` 输入渲染的图标在 NbActionComponent 模板内部，组件样式够不着——需在页面 SCSS 内用 `.header-actions` 前缀 + `::ng-deep nb-icon` 收敛作用域后覆盖。
3. **头像/身份锚点例外**：整栏允许保留一个视觉重量稍高的元素作为身份锚（如 24px 头像），其余全部更小更轻；“删光层级”是 quiet 的大忌。
4. **紧凑档位优先**：筛选/工具栏控件一律 `size="small"`/`fieldSize="small"`，间距统一 0.25~0.625rem；图标间距全局一致。

#### C. 克制的颜色（Color Discipline）

1. **中性主导**：界面 90% 面积用 background/text-hint/text-basic 中性层；颜色只出现在“状态”上（active/成功/警告/危险/未读点）。
2. **图标默认 muted**：工具栏图标默认 `text-hint-color`，hover/active 才提亮或用主题色；禁止 info/primary/warning 色作装饰（如彩色图标墙）。
3. **柔和主题色**：激活/选中用 `color-primary-transparent-100/200`（8%/16% 主题自适应透明主色），不用近白的 `--color-primary-100`（dark/cosmic 下是白色）。
4. **统计弱化**：计数、指标等元信息用纯文字 hint 色（`1角色·0全局`），不用彩色图标墙、胶囊徽章堆砌。

#### C2. 列表页卡头（页头信息审计）

列表页卡片头**默认不含文字标题**：

1. **信息审计后再决定去留**：页面名已在 Tab 页签/侧栏菜单出现、集群名在顶栏 cluster-selector 全局常驻——卡头重复展示属冗余，整组删除（含 `|` 分隔符、`<strong>` 加粗、内联 `font-size:13px`）。
2. **仅保留独有信息**：如“N 个变量”计数这类无处可查的状态，移入右侧工具栏（`count-label`），不为它保留整个标题组。
3. **布局锚点**：删标题后卡头用 `justify-content-end`（唯一子元素才不会贴左）；dialog 的“标题 + 关闭按钮”双元素结构仍用 `justify-between`，不要混淆。
4. **标题层级**：保留标题的场景（多卡分区、dialog）一律 `h6`（1.125rem），不用 `h5`（1.375rem）压过内容。

#### D. 语义色与主题变量（Semantic Tokens）

1. 颜色一律 `nb-theme()/var(--*)`；Menu/Tab 等覆盖优先在 `themes.scss` 用 Nebular 官方变量扩展（如 `menu-item-active-background-color: color-primary-transparent-200`），而非手写 CSS。
2. 语义化变量分层：surface 三层（`background-basic-color-1/2/3`）、border、text-hint/basic；同一元素内不混用背景层与边框层的同名变量。
3. 删除自定义后回归原生默认（如菜单栏 5 色图标、左侧竖线全删，回归 Nebular 默认 muted 图标 + 主色文字）——“删掉自定义”也是优化。

#### E. 层级与动效（Hierarchy & Motion）

1. **层级靠字重/字号/留白**，不靠颜色与加粗堆叠；一屏只允许一个视觉锚点。
2. 微交互 150~200ms、ease-out；transform/opacity only；hover 浮起 ≤1px 位移。
3. 动效只表达状态变化，无装饰性动画。

#### F. Angular/Nebular 特定陷阱（写样式前必查）

1. **`:host.<class>` 编译陷阱**：Angular 将 `:host.overflow` 编译为“组件内部后代 .overflow”，永远不匹配宿主——状态 class 应绑定到内部真实元素（如 `.tab-bar.overflow`）。
2. **`Set(NodeList)` 类型**：`new Set(document.querySelectorAll(...))` 推导为 `Set<unknown>`，须 `new Set<HTMLElement>(Array.from(...))`。
3. **容器查询优先级**：`@container` 外层单类会被内层双类选择器覆盖而失效；跨组件样式覆盖需对齐特异性层级。
4. **尺寸测量去污染**：用 `scrollWidth/offsetWidth` 判断溢出时，必须先移除折叠/压缩样式再测量，否则折叠宽度会反向污染判断；用 `ResizeObserver` 观察每个子项以捕获字体/图标后到导致的宽度变化。
5. **LocalDataSource 就绪前 load 丢首行**：表格由 `@if` 延迟创建时，API 先返回则首条被吞；表格常驻渲染 + 首次 load 放 `ngAfterViewInit`。

## 2. 状态（缺一不可）

每个异步区块必须有：loading（骨架或 spinner，必有最短感知帧/超时兜底 20s）、
error（精确原因 + 重试）、empty（插图/文案 + 行动按钮）。使用 `nbSpinner` 时，在异步收尾（含表格赋值失败）统一释放 loading 并触发变更检测，禁止残留遮罩拦截交互。

## 3. 反馈与动效

- 微交互 150~300ms，只用 transform/opacity。
- 危险/写操作：确认弹窗；成功/失败 toast 说明结果。
- 流式内容给光标；打断（停止）保留已到文本 + 标记。

## 4. 对比度与可达

- 正文/ hint 文案对比度 ≥4.5:1；图标按钮一律 title/tooltip；表单有 label；
- 键盘：Enter 发送（IME 组词中不触发）、Esc 关弹窗/面板。

## 5. 文案

中文、动词开头、按钮名与结果 toast 同词（“发布”→“已发布”）；错误说清问题+恢复办法，不道歉不含糊。

## 6. 反模式（本项目出现过，禁止回归）

- 原生 `prompt()/confirm()` 做输入（必须用 Nebular dialog）。
- 不存在的 Eva 图标名（用前查 `outline-icons.json`）。
- 无归属的裸 `nb-theme()` 之外的色值；跟随 OS 而非主题的第三方样式。
- `display:block` 破坏表格 thead/tbody 对齐；自闭合 SVG 误报式检查。

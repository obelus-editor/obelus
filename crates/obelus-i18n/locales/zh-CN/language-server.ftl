## Obelus 对语言服务器以及它的回答所说的话。

# 没有自己程序名的语言服务器怎么称呼。
the-language-server = 语言服务器
no-server-for-this-file = 此文件没有语言服务器
no-server-to-ask = 没有可询问的语言服务器
not-installed = 未安装 { $command }
servers-are-off = 语言服务器已在设置中关闭
no-server-running-for = 没有为 { $language } 运行的语言服务器
no-server-for = 没有适用于 { $language } 的语言服务器
still-starting = { $server } 仍在启动
not-listening = 无法联系语言服务器
did-not-answer = 语言服务器没有回应
not-responding = { $command } 没有响应
would-not-start = 无法启动 { $command }
was-not-running = { $command } 没有在运行
no-file-to-restart = 没有打开的文件，无法重启语言服务器
no-file-to-stop = 没有打开的文件，无法停止语言服务器
# 语言服务器返回了错误，但没有附带说明。
no-message = 语言服务器报告了错误，但没有说明原因
still-indexing = 仍在建立索引
asking = 正在询问语言服务器…
changed-while-asking = 等待回应期间文件已更改
not-a-file-to-change = 此文件不可编辑

## 语言服务器不提供的功能。

no-completions = { $server } 不提供补全
no-signatures = { $server } 不提供参数提示
no-code-actions = { $server } 不提供代码操作
no-hover = { $server } 不提供悬停信息
no-renaming = { $server } 不支持重命名
answers-none-of-these = 语言服务器不支持这些操作

## 代码操作。

asking-for-actions = 正在获取代码操作…
no-actions-here = 这里没有可用的代码操作
# 冒号后面是语言服务器自己给出的原因。
no-actions-because = 这里没有可用的代码操作：{ $reasons }
code-actions = 代码操作
filter-code-actions = 筛选代码操作
left-unfinished = 语言服务器没有补全这个操作
could-not-resolve = 语言服务器无法解析这个操作
# 操作的标题，按语言服务器写的原样。
did-nothing = “{ $action }”没有产生任何效果
# 命令的标题，按语言服务器写的原样。
asked-to-run = 已请求语言服务器执行：{ $command }
running-actions-on-save = 正在执行代码操作…

## 语言服务器要求的编辑。

nothing-to-change = 没有需要更改的内容
changed-files = 已更改 { $changed } 个文件，尚未保存
# 被更改的文件中有些原本没有打开，现在打开了。
changed-and-opened = 已更改 { $changed } 个文件（新打开 { $opened } 个），尚未保存
not-changed = { $failed } 个文件无法更改
# Obelus 拒绝做的事，列在冒号后面。
will-not = Obelus 不会执行：{ $what }
creating-a-file = 创建文件
moving-a-file = 移动文件
deleting-a-file = 删除文件
# 句子里列举多项时的分隔符。
list-separator = 、

## 重命名。

not-on-a-name = 光标不在名称上
renaming-to = 正在重命名为 { $name }…
changed-while-renaming = 重命名期间文件已更改
renamed-nothing = 语言服务器没有重命名任何内容
no-file-to-rename = 没有可重命名的文件
checking-the-rename = 正在确认重命名 { $path } 会影响哪些内容…
not-ready-to-update = 语言服务器尚未就绪，没有更新其他内容
could-not-rename = 无法重命名：{ $error }
renamed-to = 已重命名为 { $path }

## 符号、位置和大纲。

no-symbol-here = 这里没有符号
# 关于符号的提问，用它的命令标题。
nothing-found = { $action }：没有结果
filter-places = 筛选位置
no-file-to-outline = 没有可显示大纲的文件
filter-symbols = 筛选符号
no-symbols-in-this-file = 此文件中没有符号
no-outline-for-this-language = 这种语言没有大纲
changed-while-formatting = 格式化期间文件已更改
no-symbol-matches = 没有匹配的符号

## 调用。

nothing-to-follow = 这里没有可追踪的调用
filter-calls = 筛选调用
callers = 传入调用
calls = 传出调用
nothing-calls-that = 没有地方调用它
calls-nothing = 它没有调用任何内容

## 问题。

no-problems-in-this-file = 此文件中没有问题
no-problems-reported = 此项目中没有报告的问题
filter-problems = 筛选问题
tab-file = 文件
tab-project = 项目
errors = { $count } 个错误
warnings = { $count } 个警告
infos = { $count } 条信息
hints = { $count } 个提示
# 问题列表上方：每种各有多少，然后是在哪里。
problems-in-file = { $file } 中有 { $problems }
problems-in-files = { $files } 个文件中有 { $problems }

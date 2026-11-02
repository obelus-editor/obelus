## 复制和剪切：状态栏告诉读者拿到了什么。

copied-selection = 已复制所选内容
copied-line = 已复制当前行
copied-code = 已复制代码
copied-message = 已复制消息
copied-note = 已复制笔记
copied-answer = 已复制回答
# 聊天设置里 Slack 的应用清单。
copied-manifest = 已复制应用清单
not-copied-selection = 无法复制所选内容
not-copied-line = 无法复制当前行
not-copied-code = 无法复制代码
not-copied-message = 无法复制消息
not-copied-note = 无法复制笔记
not-copied-answer = 无法复制回答
not-copied-manifest = 无法复制应用清单
cut-selection = 已剪切所选内容
cut-line = 已剪切当前行
cut-code = 已剪切代码
cut-message = 已剪切消息
cut-note = 已剪切笔记
cut-answer = 已剪切回答
cut-manifest = 已剪切应用清单
# 文字已经从原处拿走，但剪贴板没有接收。
cut-not-copied-selection = 已剪切所选内容，但未能放入剪贴板
cut-not-copied-line = 已剪切当前行，但未能放入剪贴板
cut-not-copied-code = 已剪切代码，但未能放入剪贴板
cut-not-copied-message = 已剪切消息，但未能放入剪贴板
cut-not-copied-note = 已剪切笔记，但未能放入剪贴板
cut-not-copied-answer = 已剪切回答，但未能放入剪贴板
cut-not-copied-manifest = 已剪切应用清单，但未能放入剪贴板

## 粘贴。

nothing-to-paste = 没有可粘贴的内容
# 从连接另一端的机器上粘贴来的图片。
bringing-the-picture = 正在传输图片
takes-no-pictures = 这个 agent 不接受图片
pictures-go-to-agents = 图片只能粘贴到发给 agent 的消息里

## 跳转。

could-not-open = 无法打开 { $path }
nowhere-further-back = 已经是最早的位置
nowhere-further-forward = 已经是最新的位置
no-file-open = 没有打开的文件
unknown-language = Obelus 不认识这种语言
no-bracket-here = 这里没有括号
nothing-wider-to-select = 选区已无法再扩大
# 转到行时输入的内容，带引号。
not-a-line-number = { $text } 不是行号

## 编辑。

nothing-to-undo = 没有可撤销的操作
nothing-to-redo = 没有可恢复的操作
no-line-comment = 这种语言没有行注释

## 列表。

no-theme-is-built-in = 没有内置主题
filter-encodings = 筛选编码
# 选择了一种无法用来重新读取文件的编码之后。
written-as = 之后将以 { $encoding } 保存
filter-commands = 筛选命令
# 命令面板的第一个分页：它下面那个视图的按键。
here = 当前
# 一对键中的一个，用这对键所做的事来称呼。
does-up = 向上{ $does }
does-down = 向下{ $does }
does-left = 向左{ $does }
does-right = 向右{ $does }
no-log-file = 没有日志文件
no-server-log-file = 没有语言服务器日志文件
filter-files = 筛选文件
no-changes = 没有更改
no-files-in-this-folder = 此文件夹中没有文件
# 已更改的文件是子模块时，显示在旁边。
submodule = 子模块
# 设置由项目的设置文件决定时。
set-by = 由 { $path } 设定
# agent 还没有命名的对话。
a-conversation = 对话

## 在丢失内容之前询问。

unsaved-file = { $path } 尚未保存
unsaved-files = { $files } 个文件尚未保存
unsaved-file-and-terminals = { $path } 尚未保存，还有 { $terminals } 个终端在运行
unsaved-files-and-terminals = { $files } 个文件尚未保存，还有 { $terminals } 个终端在运行
save-and-close = 保存并关闭
close-without-saving = 不保存并关闭
save-all-and-quit = 全部保存并退出
quit-without-saving = 不保存并退出
save-all-and-switch = 全部保存并切换
switch-without-saving = 不保存并切换
save-all-and-close = 全部保存并关闭
save-all-and-close-others = 全部保存并关闭其他
close-others-without-saving = 不保存并关闭其他
changes-will-be-lost = 更改将会丢失
changed-on-disk = { $path } 已在磁盘上更改
overwrite = 覆盖
overwrite-about = 磁盘上的版本将会丢失
use-the-disk = 使用磁盘上的版本
use-the-disk-about = 撤销可以找回你的版本
deleted = { $path } 已被删除
write-it-back = 重新写回

## 打开和保存。

no-longer-exists = { $path } 已不存在
is-a-directory = { $path } 是一个目录
could-not-open-because = 无法打开 { $path }：{ $error }
outside-the-project = { $path } 不在项目中
not-a-directory = { $path } 不是目录
could-not-create = 无法创建 { $path }
could-not-create-because = 无法创建 { $path }：{ $error }
already-exists = { $path } 已存在
no-file-to-close = 没有可关闭的文件
nothing-to-preview = 这个文件没有可预览的内容
no-file-to-preview = 没有可预览的文件
is-a-plan = 这是 agent 的计划，不是文件
is-a-commits-version = 这是某次提交中的版本，不是文件本身
nothing-to-save = 没有需要保存的更改
formatting = 正在格式化…
reverted = 已恢复为磁盘上的版本，撤销可以找回你的版本
could-not-read-the-file = 无法读取文件
# 文件所用的编码无法表示其中某个字符。
not-saved-unencodable = 无法保存 { $name }：{ $encoding } 无法表示 { $character }
not-saved-named = 无法保存 { $name }
not-saved = 无法保存

## 搜索。

search-this-file = 搜索此文件
search-this-preview = 搜索此预览
search-this-conversation = 搜索此对话
search-this-page = 搜索此页面
search-the-notes = 搜索笔记
type-to-search-this-file = 输入内容以搜索此文件
type-to-search-this-preview = 输入内容以搜索此预览
type-to-search-this-conversation = 输入内容以搜索此对话
type-to-search-this-page = 输入内容以搜索此页面
type-to-search-the-notes = 输入内容以搜索笔记
no-match-in-this-file = 此文件中没有匹配项
no-match-in-this-preview = 此预览中没有匹配项
no-match-in-this-conversation = 此对话中没有匹配项
no-match-on-this-page = 此页面上没有匹配项
no-match-in-the-notes = 笔记中没有匹配项
nothing-here-to-search = 这里没有可搜索的内容
nowhere-to-search = 没有可搜索的位置
no-match-in-the-project = 项目中没有匹配项
not-a-pattern = 无效的搜索模式
type-to-search-every-file = 输入内容以搜索所有文件
searching = 正在搜索…
type-to-search-symbols = 输入内容以搜索项目中的符号

## 项目，以及 Obelus 为它保存的东西：笔记、设置、主题、打开过的项目和机器。

## 笔记。

# 另一个窗口删除了这个窗口正在修改的笔记。
note-taken-away = 该笔记已在其他地方被删除
notes-unreadable = 无法读取笔记文件
notes-unreadable-not-written = 无法读取笔记文件，因此没有保存任何内容
notes-not-written = 无法保存笔记

## 设置文件。

not-saved-unreadable = 未保存：无法读取设置文件
# 冒号后面是系统给出的原话。
not-saved-because = 未保存：{ $error }
settings-unreadable = 无法读取设置文件，因此不会保存任何更改
nowhere-for-settings = 此系统上没有存放设置文件的位置
settings-file-not-written = 无法创建设置文件：{ $error }

## 设置文件中没有起作用的行，标在那一行上。名字用反引号括起来，像编译器那样。

no-such-setting = 没有名为 `{ $key }` 的设置
not-for-a-project = 项目不能设置 `{ $key }`
not-a-table = `{ $key }` 的值必须是一个表
no-such-choice = 不是可选的值：`{ $word }`
no-such-theme = 没有名为 `{ $theme }` 的主题
no-such-command = 没有名为 `{ $command }` 的命令
no-such-key = 没有名为 `{ $key }` 的按键
# 冒号后面是这个键不能绑定的原因。
key-not-allowed = `{ $key }` 不能绑定命令：{ $why }

## 设置的值，按设置页的说法。

# 读者没有选择时，agent 自己的设置所用的值。
agents-own-value = agent 开始对话时自己的默认值
# 列表中当前所用的值旁边。
current = 当前
colours-auto = 自动
colours-truecolor = 24 位真彩色
colours-256 = 256 色
jobs-unlimited = 不限
jobs-all = 全部 { $cpus } 个 CPU
jobs-half = 一半：{ $cpus } 个 CPU 中的 { $jobs } 个
# 对话保留多久：0 表示全部保留。
days-forever = 永久
days-week = 一周
days-month = 一个月
days-three-months = 三个月
days-year = 一年
days = { $days } 天
filter-fonts = 筛选本机字体

## 键盘映射。

choose-a-keymap = 选择键盘映射
# 键盘映射是什么，然后是它把两件事放在哪个键上。
layout-gloss = { $gloss }。{ $keys }
layout-keys = { $open } 打开文件，{ $run } 运行命令
layout-open-key = { $open } 打开文件
layout-run-key = { $run } 运行命令
# 用键盘映射的名字。
reset-every-key-to = 要把所有快捷键重置为 { $layout } 的吗？
reset-every-key = 要把所有快捷键重置为键盘映射的默认值吗？
reset-them = 重置
keep-them = 保留

## 主题。

# 主题的来源，显示在名字旁边：读者自己的，或从主题库下载的。内置主题不显示。
theme-local = 本地
theme-downloaded = 已下载
more-themes = 更多主题
no-theme-by-that-name = 没有这个名称的主题。按 Enter 在“更多主题”中查找
nowhere-to-keep-themes = 此系统上没有可存放主题的位置
theme-unreadable = 无法读取主题 { $theme }
# 标在主题文件上，在解析器的原话上面。
this-theme-unreadable = 无法读取这个主题
# 主题库已不再列出的主题一删就没了，所以要按两次。
delete-again = 再删除一次即可彻底移除 { $theme }

## 项目和机器。

projects-unreadable = 无法读取项目列表：{ $why }
filter-projects = 筛选项目
no-other-projects = 还没有打开过其他项目
filter-this-folder = 筛选此文件夹
new-connection = 新建连接
filter-machines = 筛选机器
# 引号里是 `ob --server` 打印的英文原话。
not-a-server-key = 这不是服务器密钥：密钥是 “gets in with the key” 后面的那串字母

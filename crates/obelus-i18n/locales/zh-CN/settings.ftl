## 设置页的分页，以及按键页的标题。

group-appearance = 外观
group-reading = 阅读
group-files = 文件
group-agent = Agent
group-keys = 按键

## 每个设置的名字，按它在文件里的键（下划线换成连字符）；需要解释的，在
## `-about` 里说它做什么。用中文编辑器设置里惯用的说法，写短。

theme = 主题
icons = Nerd Font 图标
icons-about = 终端字体需要是 Nerd Font，否则图标会显示为方框
language = 语言
wrap = 自动换行
blame-margin = Git 追溯
blame-margin-about = 显示在光标所在行的末尾
tab-width = 制表符宽度
tab-width-about = 也是按 Tab 键插入的空格数
fonts = 字体
fonts-about = 按顺序依次尝试
animation = 动画
bar-tabs = 标题栏标签页
bar-tabs-about = 标题栏会因此变高
font-size = 字号
font-size-about = 单位为磅
picture-strength = 背景图片强度
picture-strength-about = 相对主题自身设定的百分比
hover-delay = 悬停延迟
hover-delay-about = 单位为毫秒，设为 0 则关闭
read-only = 只读
read-only-about = agent 仍可写入
language-servers = 语言服务器
language-servers-about = 打开文件时自动启动。关闭后仍可用 restart-server 手动启动
format-on-save = 保存时格式化
code-actions-on-save = 保存时执行代码操作
code-actions-on-save-about = 例如整理 import
inlay-hints = 内嵌提示
inlay-hints-about = 推断出的类型和参数名
diagnostics = 行内诊断
diagnostics-about = 在光标所在行下方显示问题信息
new-versions = 检查更新
new-versions-about = 每天一次，有新版本时在欢迎页提示
ignored-files = 显示被忽略的文件
ignored-files-about = 在文件列表中
hidden-files = 显示隐藏文件
hidden-files-about = 在文件列表中，包括 .git
reopen = 恢复打开的文件
reopen-about = 光标回到离开时的位置
recent-projects = 最近的项目
workflow = 工作流
workflow-about = agent 修改这个项目的方式
build-jobs = 并行编译任务数
build-jobs-about = Obelus 启动的程序可同时运行的编译任务上限，本机所有 Obelus 共享
speaks-as = Agent 名称
speaks-as-about = agent 在对话中的自称，这样对话里的"我"始终指你
conversation-days = 对话保留天数
conversation-days-about = 针对与笔记无关的对话，与笔记相关的会随笔记删除。设为 0 则永久保留
keys-from = 键盘映射
keys-from-about = 你自定义的快捷键以哪套为基础

## 每套键盘映射是什么。它的名字是名字，不在这里。

layout-obelus-about = 功能键，四个一组
layout-mnemonic-about = Ctrl 和 Alt 加单词首字母。在 Obelus 的终端里，这些键交给 shell
layout-vscode-about = 不使用功能键
layout-textmate-about = 不使用功能键

## `language` 设置自己的词。每种语言用它自己的名字列出，那不是消息：
## 见 `Language::name`。

language-auto = 跟随系统

## 设置页：分页、各行上的词，以及它显示的按键。设置本身的名字和说明在
## settings.ftl 里。

tab-settings = 设置
tab-keys = 快捷键
tab-agents = Agent
tab-chat = 聊天
tab-themes = 主题

# 筛选框为空时，在每个分页上的提示。
filter-keys = 筛选快捷键
filter-agents = 筛选 agent
filter-chat = 筛选聊天设置
filter-settings = 筛选设置
filter-themes = 筛选主题

no-command-matches = 没有匹配的命令
no-setting-matches = 没有匹配的设置
no-agent-matches = 没有匹配的 agent
no-theme-matches = 没有匹配的主题

# 设置值的来源，显示在值旁边。
scope-project = 项目
scope-global = 全局
scope-default = 默认
# 快捷键分页上：命令用的不是键盘映射里的键。
scope-changed = 已修改

## 按键，显示在行上和命令面板的第一个分页上：一个词，以及 `-about` 里
## 说它做什么。

change = 更改
change-key-about = 更改这个快捷键
change-setting-about = 更改这个设置
pair = 配对
pair-about = 生成与聊天配对的代码
copy = 复制
copy-about = 复制这个值
open = 打开
open-steps-about = 打开设置步骤
add = 添加
add-variable-about = 为 agent 添加环境变量
# 在框里调节一个数字。
turn = 调节
turn-about = 把数字调小或调大一档
keep = 确定
keep-about = 使用框里的数字
cancel = 取消
cancel-about = 保持原来的数字
remove = 删除
remove-key-about = 删除这个快捷键
remove-variable-about = 删除这个环境变量
remove-theme-about = 卸载这个主题
add-key = 添加快捷键
add-key-about = 为这个命令再绑定一个键
reset = 重置
reset-about = 把这个命令恢复为键盘映射里的键
reset-all = 全部重置
reset-all-about = 把所有命令恢复为键盘映射里的键
forget = 清除
forget-about = 清除这个字段保存的值
# 读者把 agent 的某个设置交给 agent 自己决定。
agents-own = 由 agent 决定
agents-own-about = 交给 agent 决定
unset = 取消设置
unset-about = 从项目的设置文件中删除这个设置
use-theme = 使用
use-theme-about = 使用这个主题
update = 更新
update-theme-about = 更新这个主题
install = 安装
install-theme-about = 安装这个主题

## 快捷键分页。

key-is-taken = { $key } 已绑定到 { $command }
press-the-new-key = 按下新的键，或按 { $key } 删除
press-a-key = 按下要添加的键

## agent 的设置。

# agent 名字下面。
when = 对新对话生效
# agent 环境变量的标题下面。
started = agent 下次启动时生效
environment-of = { $agent } · 环境变量
add-a-variable = 添加变量
add-a-variable-about = 先填名字，再填值
no-longer-offered = { $agent } 已不再提供这个选项，新对话将按它自己的默认开始

## 以卡片显示的 agent 和主题。

agents-failed = 无法获取 agent 列表：{ $why }
fetching-agents = 正在获取 agent 列表…
themes-failed = 无法获取主题列表：{ $why }
fetching-themes = 正在获取主题列表…
active = 当前使用
in-use = 使用中
installed = 已安装
no-longer-listed = 已不在列表中
installing = 正在安装…
updating = 正在更新…
installing-percent = 正在安装 { $percent }%
updating-percent = 正在更新 { $percent }%
installing-left = 正在安装 { $percent }% · 剩余 { $seconds } 秒
updating-left = 正在更新 { $percent }% · 剩余 { $seconds } 秒
wallpaper-licensed = 壁纸 { $license }
# 不允许转发的壁纸，从作者那里获取。
wallpaper-from-author = 壁纸来自作者
no-wallpaper = 没有壁纸
# 读者自己的同名主题文件优先。
shadowed = 正在使用你自己的 { $theme }

## 字体设置，在没有选择字体时。

system-font = 系统默认

## 聊天分页。

chat-heading = 聊天
platform = 平台
platform-about = 在哪个聊天里处理笔记
people = 成员
people-about = 谁可以通过它与这台机器对话
pair-row-about = 发到你添加了机器人的群里的代码
off = 关闭
not-set = 未设置
nobody = 无
make-a-code = 生成代码
refused-by = 被 { $platform } 拒绝
refused = 被拒绝
tokens-unreadable = 无法读取保存的令牌

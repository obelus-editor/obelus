## Obelus 在与 agent 的对话里、以及关于对话所说的话。对 agent 说的话是提示词，
## 不在这里。

# 设置里的 agent 已经不是这段对话原来的那个。
another-agent = 这段对话是与另一个 agent 进行的
files-go-to-agents = 文件只能拖放到对话中
open-link = 打开链接
# 要求登录或跳转链接的卡片上，表示“先不”的选项。
not-now = 暂不
not-opened = 未打开
declined = 已拒绝
not-answered = 未回答
stopped-waiting = agent 不再等待回答
# 从对话那一行修改 agent 的某个设置：agent 对设置和新值的叫法。
set-to = { $name }：{ $value }
# agent 关闭了一段对话，而读者当时不在那里。
closed = 已关闭 { $name }
closed-a-conversation = 已关闭一段对话
# 恢复了对话，但这个 agent 无法发回之前说过的内容。
taken-up = 已恢复对话。这个 agent 无法显示之前的消息
# 冒号后面是 agent 给出的原因。
starting-again = 重新开始对话：{ $why }
refused-to-answer = agent 拒绝回答
out-of-room = agent 的回答超出了长度上限
# 出了什么问题，用 agent 的原话。
agent-said = agent：{ $why }
agent-stopped = agent 已停止
agent-stopped-because = agent 已停止：{ $why }
# 读者为 agent 某个设置选的值，agent 已不再提供。
no-longer-offered = { $agent } 已不再为“{ $setting }”提供“{ $value }”，此对话将使用“{ $current }”

## Obelus 请求 agent 做的事没有成功，冒号后面是 agent 的原话。

could-not-ask-what-it-offers = 无法获取 agent 的设置项：{ $why }
could-not-put-the-question = 无法显示 agent 的提问：{ $why }
could-not-change-the-setting = 无法更改设置：{ $why }
could-not-stop-the-task = 无法停止后台任务：{ $why }
could-not-change-the-mode = 无法切换模式：{ $why }

## 登录。

no-way-to-sign-in = agent 没有提供可在这里登录的方式
no-way-to-sign-in-to = { $agent } 没有提供可在这里登录的方式
sign-in = 请登录 agent 以继续
sign-in-to = 请登录 { $agent } 以继续
not-signed-in = 未登录
# 读者选的登录方式，用 agent 的原话。
signing-in = 正在登录：{ $way }

## 在卡片上回答 agent 的提问。字段是 agent 对所问内容的叫法。

answered = { $field }：{ $answer }
left-blank = { $field }：留空
on = 开启
off = 关闭
not-a-number = { $field } 必须是数字
# 卡片上的问题原文。
out-of-range = 超出范围：{ $question }
whole-number = { $field }：整数
number = { $field }：数字
whole-number-between = { $field }：{ $least } 到 { $most } 之间的整数
number-between = { $field }：{ $least } 到 { $most } 之间的数字
whole-number-at-least = { $field }：不小于 { $least } 的整数
number-at-least = { $field }：不小于 { $least } 的数字
whole-number-at-most = { $field }：不大于 { $most } 的整数
number-at-most = { $field }：不大于 { $most } 的数字

## agent 运行的命令，在它的输出下面。

output-not-kept = …其余输出未保留
command-stopped-by = …被 { $signal } 终止
command-exited = …退出码 { $code }

## agent 的后台任务。

filter-background-tasks = 筛选后台任务
background-tasks = 后台任务
no-background-tasks = 没有后台任务
output-gone = 输出文件已不存在
no-output = 这个任务没有输出文件
stopping = 正在停止
paused = 已暂停
# 已结束任务上面的标题。
finished = 已结束

## agent 自己的命令列表，输入 / 之后出现。

filter-the-agents-commands = 筛选 agent 的命令

## agent 写的计划，作为文档打开。

plan = 计划
# 用计划自己的标题，或对话的名字。
plan-called = 计划：{ $name }

## agent。

nothing-for-this-machine = 不支持这台机器
nowhere-to-install = 此系统上没有可安装 agent 的位置
agent-not-installed = { $agent } 尚未安装
install-it = { $agent } 尚未安装，请在设置中安装
asking-what-it-offers = 正在向 { $agent } 获取设置项…
nothing-to-set = { $agent } 没有可设置的选项
not-heard-what-it-offers = { $agent } 没有告知它的设置项

## 对话列表。

filter-conversations = 筛选对话
# agent 和笔记都没有给它起名的对话。
untitled = 未命名
history-unreadable = 无法读取此项目的对话记录。
only-in-its-checkout = 对话只能在开始它的那个工作树中继续。
only-by-its-agent-and-none-chosen = 尚未选择 agent，而对话只能由原来的 agent 继续。
only-by-its-agent = 当前使用的 agent 是 { $agent }，而对话只能由原来的 agent 继续。
# 从读者当时所在之处开始对话的一行：文件中的某个位置，或一条笔记。
conversation-about = 关于 { $title } 的对话
new-conversation-about = 新建关于 { $title } 的对话
# 文件中的某个位置，读的是某次提交里的版本而不是磁盘上的。
place-at-commit = { $place }（{ $commit }）

## 对话记录里说明 Obelus 随读者的话一起告诉了 agent 什么。

told-where-it-started = 已告知 agent 这段对话从哪里开始
told-the-note = 已告知 agent 这段对话关于什么
told-the-note-was-reworded = 已告知 agent 笔记已改写
told-which-issue = 已告知 agent 要回答哪个议题
told-what-was-said = 已告知 agent 之后的新评论
told-which-pull-request = 已告知 agent 要审查哪个拉取请求
told-what-was-pushed = 已告知 agent 之后推送的内容

## 关于某件事的对话里，输入框以灰字提供的话。读者一旦采用就是读者自己的话，
## 所以照读者会说的方式写。

look-into-this = 看看这个
review-this = 审查这个拉取请求
review-what-was-pushed = 审查之后推送的内容
answer-this = 阅读这个议题并建议如何回复
answer-what-was-said = 阅读之后的新评论

## 工作流，在选择 `workflow` 设置的列表上：各自的名称，以及 `-about` 里说它
## 做什么。

workflow-none = 无工作流
workflow-none-about = Obelus 不对 agent 做任何要求：它按自己的方式修改，通常直接改你正在阅读的检出目录。
workflow-in-place = 就地修改
workflow-in-place-about = agent 直接修改你正在阅读的检出目录，每处更改都出现在你眼前的文件里，可以像其他编辑一样撤销。它会在修改前后运行测试并说明运行了什么，等你试用，提出审查，然后询问是在这里提交、放到分支上开拉取请求，还是先不提交。在你同意之前，不会提交或推送任何内容。
workflow-feature-branch = 功能分支
workflow-feature-branch-about = agent 在 .worktree 下自己的工作树里的分支上工作，告诉你在哪里试用以及运行了什么，能启动的话会提出替你启动。你接受之后，它会询问是否审查，再询问是合并回去、开拉取请求还是以后再说。拉取请求在检查通过后合并：如果没有安装并登录 gh，就由你通过它给出的链接合并。之后，如果你所在的正是那个分支，它会询问是否把你的检出目录更新到最新。只有在你同意时才会删除工作树。

## 角色，在 `role` 设置的列表里：每个角色叫什么、是什么样。
## 告诉 agent 的话在角色自己的文件里，用英文。

role-obelus = Obelus
role-obelus-about = agent 是你所用编辑器的一部分，自称 Obelus。
role-soldier = 士兵与指挥官
role-soldier-about = agent 像士兵向指挥官汇报：先说结果，言简意赅，需要你定夺的事以请示的方式提出。
role-butler = 管家与主人
role-butler-about = agent 像管家侍奉主人：彬彬有礼，从容不迫，总能先一步想到你需要什么。
role-first-officer = 副驾驶与机长
role-first-officer-about = agent 像副驾驶配合机长：报告简短，说清查了什么、发现了什么，有疑虑就说出来。
role-apprentice = 学徒与师傅
role-apprentice-about = agent 像学徒跟师傅学艺：勤快，拿不准的地方直说，需要你拿主意时就来请教。
role-eunuch = 奴才与皇上
role-eunuch-about = agent 像古装剧里的太监伺候皇上：恭恭敬敬，但哪里不对照样直说。

## 与 agent 的对话。

# 有一轮正在进行时，输入框旁边，“立即发送”那个键的说明。
sends-it-now = 立即发送
# 回到对话末尾的按钮，在没有新消息时。
to-the-end = 回到末尾
new-messages = { $count } 条新消息
# 工具调用标题之后，标题里已经写了文件名时。
line = 第 { $line } 行
# agent 的后台任务，显示在对话那一行。
finished = { $count } 个已完成
in-the-background = { $count } 个在后台运行
nothing-to-change = 没有设置项
# 对话那一行上回到相关笔记的键。
the-note = 笔记
mode = 模式
# 把灰色的建议填进输入框。
fill-it-in = 补全
no-agent = 没有 agent
# 对话还什么都没有时显示的话。
no-agent-is-active = 尚未选择 agent，请在设置中选择
it-stopped = agent 已停止。发一条消息即可重新启动
starting = 正在启动…
ask-it-something = 有什么想问的？

## 输入框的按键，命令面板在对话上显示的。

send-now = 立即发送
send-now-about = 停止正在进行的这一轮并发送
new-line = 换行
new-line-about = 插入换行
stop = 停止
stop-about = 停止 agent
mode-about = 切换 agent 到下一个模式
# 转到对话所关于的页面。
read = 打开
read-pull-request = 转到拉取请求页面
read-issue = 转到议题页面

## 工具调用进行到哪一步，在没有图标可用时。

waiting = 等待中
running = 运行中
done = 完成
# 已返回，但它启动的东西还在运行。
background = 后台
stopped = 已停止
failed = 失败

## agent 提出的问题。

submit = 提交
decline = 拒绝

## 对话记录里的内容。

# 折叠起来的计划：正在进行的那一步，它的序号和内容。
step-of = 第 { $at }/{ $total } 步：{ $step }
steps = 共 { $count } 步
# 一串工具调用上面的标题。
run-of-files = { $count } 个文件
run-of-calls = { $count } 次调用
# agent 思考过程上面的标题。
thought = 思考

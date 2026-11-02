## 作为页面阅读的拉取请求或议题。

# 开始一段关于它的对话的键。
talk = 询问 agent
talk-about-pull-request = 让 agent 审查这个拉取请求
talk-about-issue = 让 agent 回答这个议题
files = { $count } 个文件
asking-about-pull-request = 正在从 GitHub 获取描述、检查和评论
asking-about-issue = 正在从 GitHub 获取描述和评论
no-description = 没有描述
files-heading = 文件
checks-heading = 检查
# 标题后面，没有文件或没有检查时。
none = 无
# 文件列表下面，GitHub 只列出了一部分时。
more-on-github = 另有 { $count } 个，见 GitHub
checks-failed = { $count } 个失败
checks-running = { $count } 个运行中
checks-passed = { $count } 个通过
checks-skipped = { $count } 个跳过
no-comments = 没有评论
# 评论作者和评论所在文件之间。
on = 评论于
replies = { $count } 条回复
resolved = 已解决
outdated = 已过时

## 为什么 GitHub 那边什么都没有。

no-gh = 列出拉取请求需要 gh，但它没有安装
signed-out = 尚未登录 GitHub。运行 gh auth login 登录
github-failed = GitHub 没有响应：{ $why }
# GitHub 没有响应的原因，跟在上面那句的冒号后面。
answer-unreadable = 无法解析它的回答
answer-unreadable-because = 无法解析它的回答：{ $error }
no-reason = 它没有给出原因

## 评论作者做了什么，跟在名字后面。

commented = 发表了评论
approved = 批准了
requested-changes = 请求更改
dismissed = 被驳回

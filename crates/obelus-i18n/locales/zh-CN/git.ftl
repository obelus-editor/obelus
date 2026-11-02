## Git 历史、工作树，以及 GitHub 上的拉取请求和议题。

nothing-changed-here = 这里没有更改

## 历史。

# 分页：正在阅读的文件的历史、分支和标签、整个项目的历史。
tab-this-file = 此文件
tab-refs = 引用
tab-the-project = 项目
no-history-here = 这里没有历史记录
filter-commits = 筛选提交
no-refs-here = 这里没有引用
reading-the-history = 正在读取历史记录…
no-commit-touches-this-file = 没有提交涉及此文件
no-commits-here = 这里没有提交
# 遍历历史进行到哪里了。
commits-read = 已读取 { $count } 个提交…
not-at-that-commit = 无法读取该提交中的这个文件

## 已打开的文档列表，以及工作树。

tab-documents = 文档
tab-worktrees = 工作树
nothing-open = 没有打开的内容
filter-open-documents = 筛选已打开的文档
no-worktrees = 没有工作树
filter-worktrees = 筛选工作树
# git 记录着、但目录已不存在的工作树。
missing = 已丢失

## 拉取请求和议题。

tab-pull-requests = 拉取请求
tab-issues = 议题
# 在已打开文档列表中，拉取请求或议题页面的旁边。
pull-request = 拉取请求
issue = 议题
filter-by-title-or-number = 按标题或编号筛选
no-open-pull-requests = 没有打开的拉取请求
no-open-issues = 没有打开的议题
asking-github = 正在询问 GitHub…
still-asking-github = 仍在等待 GitHub
# 输入的内容，带引号。
searching-github = 正在 GitHub 上搜索“{ $query }”
asking-for-more = 正在从 GitHub 再获取 { $count } 个
asking-what-changed = 正在向 GitHub 查询有哪些变化
# 列表可能不是 GitHub 当前状态的原因，在圆点后面。
may-be-out-of-date = 可能不是最新 · { $why }
open-count = { $count } 个打开
open-of-total = { $total } 个打开，已列出 { $listed } 个
matches = { $count } 个匹配
matches-so-far = 目前 { $count } 个匹配
# GitHub 的搜索最多给出一千个，不管它统计了多少。
matches-and-no-more = { $count } 个匹配 · GitHub 不再给出更多
draft = 草稿
# agent 起名之前，对话的名字。
review = 审查 #{ $number }
review-named = 审查 #{ $number }：{ $title }
issue-named = 议题 #{ $number }：{ $title }
no-commit-from-github = GitHub 没有说明它位于哪个提交
# 拉取请求中不是文本的文件。
not-text = { $path } 不是文本文件

## 用来处理笔记的聊天：Slack 或飞书。平台的名字是名字，不在这里。发到平台上的
## 话也不在这里：这些是 Obelus 在自己屏幕上说的话。

not-connected = 未连接到 { $platform }
# 冒号后面是原因：下面的某一条，或平台自己的说法。
not-connected-because = 未连接到 { $platform }：{ $why }
token-not-kept = 无法保存令牌：{ $why }
# 把另一行的令牌粘贴到了这一行：每种令牌的开头都不一样。
starts-with = { $field } 必须以 { $prefix } 开头
nowhere-to-hold = 无法占用 { $platform } 连接：没有地方存放它的锁
could-not-take-over = 无法从另一个窗口接管 { $platform } 连接
not-let-go = 另一个窗口不肯释放 { $platform } 连接

## 在对话里说：这段对话在平台上的话题漏掉了什么。

thread-not-started-in-time = 未能及时连接到 { $platform }，没有为此对话创建话题
thread-refused = { $platform } 拒绝为此对话创建话题
# 冒号后面是平台给出的原因。
message-refused = { $platform } 拒绝了此对话的一条消息：{ $why }
message-not-sent-in-time = 未能及时连接到 { $platform }，此对话的一条消息没有发出

## 为什么没有连接上。跟在冒号后面。

# 没有填写的字段，列在一起。
not-set = 未设置 { $fields }
no-answer-in-time = 没有及时响应
# 响应的状态，按网络上的写法：“401 Unauthorized”。
answered = 返回了 { $status }
no-address = 没有可连接的地址
nowhere-to-keep-tokens = 此机器上没有可保存令牌的位置
not-toml = { $path } 不是有效的 TOML
not-toml-at = { $path } 不是有效的 TOML（第 { $line } 行）

## 聊天设置页上，每个平台需要填写的内容：这一行叫什么，以及 `-about` 里说在
## 哪里找到它。提到的页面是平台自己的，用平台的叫法。

slack-app-token = App 令牌
slack-app-token-about = Socket Mode 的令牌，在 Basic Information 页面
slack-bot-token = Bot 令牌
slack-bot-token-about = 安装应用后，在 OAuth & Permissions 页面
slack-manifest = 应用清单
slack-manifest-about = 每台机器一个应用：粘贴到 Create New App → From a manifest
feishu-app-id = App ID
feishu-app-id-about = 在应用页面的“凭证与基础信息”中
feishu-app-secret = App Secret
feishu-app-secret-about = 在同一页面，App ID 旁边
feishu-domain = 域名
feishu-domain-about = 飞书填 feishu，海外版 Lark 填 lark
feishu-console = 开发者后台
feishu-console-about = 每台机器一个应用：企业自建应用，开启机器人能力，事件和卡片回调都使用长连接

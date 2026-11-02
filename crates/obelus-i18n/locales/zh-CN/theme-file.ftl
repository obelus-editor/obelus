## 主题文件中 Obelus 用不上的内容，标在那一行上。每条都说明 Obelus 改用了什么；
## 有第二行的，说明这一行应该怎么写。

could-not-read = 无法读取 { $path }：{ $error }
wallpaper-not-a-table = wallpaper 必须是一个单独的表
no-such-wallpaper-key = wallpaper 中没有这一项
no-file = 这个 wallpaper 没有指定 file
file-in-quotes = file 的值要用引号括起来
no-picture-at = { $path } 处没有图片
# 改用的不透明度。
opacity-range = opacity 必须是 0 到 100 之间的整数，因此使用 { $opacity }
# fit 或 anchor 可以取的值，列在一起。
no-such-fit =
    没有这个 fit，因此使用 cover
    可选：{ $choices }
no-such-anchor =
    没有这个 anchor，因此使用 center
    可选：{ $choices }
no-such-base = 没有这个内置主题，因此以 dark 为基础
not-a-colour =
    不是颜色，因此沿用基础主题的颜色
    颜色写作 #rrggbb 或 #rgb
no-such-colour-here = 这里没有这个颜色
colours-not-a-table = 这些颜色必须是一个单独的表
no-such-colour = 主题中没有这个颜色
italic-not-a-list =
    不是种类的列表，因此沿用基础主题的斜体
    列表写作 ["comment"]
italic-names-no-colour =
    这里有的词不是颜色名，因此没有任何内容显示为斜体
    种类的名字和它的颜色名相同，比如 comment 或 type_name

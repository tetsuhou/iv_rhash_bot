# iv_rhash_bot

Telegram官方提供了Instant View（下记IV）功能，通过对一个网站编写模板，可以让该网站在Telegram手机客户端上快速浏览内容，但是非竞赛期间官方似乎不会将个人的模板设置为默认的模板，所以为了更方便地使用这个功能，开发了这个 Telegram Bot。

## 功能

* 将输入的IV链接换成超链接，缩短消息
* 将网站链接转换成包含IV超链接的消息（需要上一条记录的rhash信息）
* 支持Inline模式

## 部署

```
docker run -d -e BOT_TOKEN=*YOUR_BOT_TOKEN* -v *YOUR_DATA_FOLDER*:/data/ ghcr.io/tetsuhou/iv_rhash_bot:main
```
或参考docker-compose.yaml

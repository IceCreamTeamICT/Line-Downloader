# Line Downloader
`Line Downloader` 是一个使用 `Rust` 编写的简单异步下载器，在本地服务器上提供下载进度浏览。

# 许可证
遵循本项目使用的 `Apache-2.0` 协议

# 构建
直接使用
```Powershell
cd LineDownloader
```
```
cargo build --release
```

# 使用
```Powershell
LineDownloader.exe {downloads.json的绝对路径} {最大协程数（可选，默认为 32）}
```

# downloads.json
格式：
```
filePath: {
    "url",
    "sha1"
}
```


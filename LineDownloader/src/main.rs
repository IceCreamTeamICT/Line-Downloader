use std::time::Instant;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::path::PathBuf;
use sha1::{Sha1, Digest};
use serde::Deserialize;
use tokio::sync::Semaphore;
use futures::future::join_all;
use tokio::time::{timeout, Duration};
use tokio::fs;  
use reqwest::Client;
use tokio::io::AsyncReadExt;  
use num_cpus;
use std::env;
use warp::Filter;
use webbrowser;

#[derive(Deserialize)]
struct FileInfo {
    url: String,
    sha1: String,
}

async fn download_with_retries(client: &Client, url: &str, path: &PathBuf, expected_sha1: &str, retries: usize) -> Result<(), Box<dyn std::error::Error>> {
    for attempt in 0..retries {
        let response = match timeout(Duration::from_secs(15), client.get(url).send()).await {
            Ok(res) => res?,
            Err(_) => {
                eprintln!("Request timed out for {}", url);
                continue;
            }
        };

        if response.status().is_success() {
            let bytes = response.bytes().await?;
            fs::write(path, &bytes).await?;

            if file_exists_and_valid(path, expected_sha1).await {
                return Ok(());
            }
        } else {
            eprintln!("Error downloading {}: {}", url, response.status());
        }

        println!("Attempt {} failed, retrying...", attempt + 1);
    }

    Err("Max retries reached".into())
}

async fn file_exists_and_valid(path: &PathBuf, expected_sha1: &str) -> bool {
    if path.exists() {
        let mut file = fs::File::open(path).await.expect("Unable to open file");
        let mut hasher = Sha1::new();
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer).await.expect("Unable to read file");
        hasher.update(&buffer);
        let result = hasher.finalize();
        let sha1_hex = format!("{:x}", result);

        return sha1_hex == expected_sha1;
    }
    false
}

async fn process_downloads(downloads: HashMap<String, FileInfo>, max_concurrent: usize, client: Client, remaining_files: Arc<Mutex<usize>>) {
    let semaphore: Arc<Semaphore> = Arc::new(Semaphore::new(max_concurrent));
    let total_files: usize = downloads.len();
    println!("Total Files: {}", total_files);

    let tasks: Vec<_> = downloads.into_iter().map(|(filename, file_info)| {
        let permit: Arc<Semaphore> = Arc::clone(&semaphore);
        let path: PathBuf = PathBuf::from(&filename);
        let url: String = file_info.url.clone();
        let expected_sha1: String = file_info.sha1;
        let remaining_files: Arc<Mutex<usize>> = Arc::clone(&remaining_files);
        let client: Client = client.clone();

        tokio::spawn(async move {
            let _permit = permit.acquire().await.unwrap();

            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).await.expect("Unable to create directories");
            }

            let result: Result<(), tokio::time::error::Elapsed> = timeout(Duration::from_secs(180), async {
                if file_exists_and_valid(&path, &expected_sha1).await {
                    println!("File already exists and is valid: {}", path.display());
                } else {
                    if path.exists() {
                        fs::remove_file(&path).await.expect("Unable to delete file");
                    }
                    download_with_retries(&client, &url, &path, &expected_sha1, 10).await.expect("Download failed");
                }
            }).await;

            if result.is_err() {
                eprintln!("Task for {} timed out", filename);
            }

            let mut remaining = remaining_files.lock().unwrap();
            *remaining -= 1;
            println!("Remaining files: {}", *remaining);
        })
    }).collect();

    join_all(tasks).await;
}

fn build_client() -> Client {
    reqwest::Client::builder()
        .pool_max_idle_per_host(32)
        .build()
        .expect("Failed to build HTTP client")
}

#[tokio::main]
async fn main() {
    println!("Debug: CPU Core: {}", num_cpus::get());
    
    let args: Vec<String> = env::args().collect();
    let downloads_path = PathBuf::from(args[1].clone());
    let downloads_json = fs::read_to_string(downloads_path).await.expect("Unable to read file");
    let downloads: HashMap<String, FileInfo> = serde_json::from_str(&downloads_json).expect("JSON was not well-formatted");

    let client = build_client();
    let start = Instant::now();
    let total_files = downloads.len();
    let remaining_files = Arc::new(Mutex::new(total_files));

    // 提供静态文件服务
    let static_files = warp::path("static")
        .and(warp::fs::dir("./static"));

    // 提供下载状态路由
    let remaining_files_clone = Arc::clone(&remaining_files);
    let remaining_files_route = warp::path("status")
        .map(move || {
            let remaining = remaining_files_clone.lock().unwrap();
            format!("文件总计: {}, 剩余文件: {}", total_files, *remaining)
        });

    // 合并路由
    let routes = static_files.or(remaining_files_route);

    // 启动 HTTP 服务器
    tokio::spawn(async move {
        warp::serve(routes).run(([127, 0, 0, 1], 3030)).await;
    });

    if webbrowser::open("http://127.0.0.1:3030/static/index.html").is_ok() {
        println!("Server was started")
    } else {
        panic!("Disable to start server")
    }

    if args.len() > 2 {
        let max_concurrent = args[2].parse().unwrap();
        println!("{} concurrents used", max_concurrent);
        process_downloads(downloads, max_concurrent, client, remaining_files).await;
    } else {
        println!("32 concurrents used");
        process_downloads(downloads, 32, client, remaining_files).await;
    }
    
    let duration = start.elapsed();
    println!("Time taken for this download: {:?}", duration);
}
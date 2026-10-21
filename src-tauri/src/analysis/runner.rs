use crate::{
    model::{Job, Settings},
    store::AppState,
};
use serde_json::{json, Value};
use std::{
    fs::{self, File},
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::Duration,
};
use tauri::{AppHandle, Manager};

const SOURCES: &[(&str, &[u8])] = &[
    (
        "__init__.py",
        include_bytes!("../../resources/analysis_worker/__init__.py"),
    ),
    (
        "__main__.py",
        include_bytes!("../../resources/analysis_worker/__main__.py"),
    ),
    (
        "common.py",
        include_bytes!("../../resources/analysis_worker/common.py"),
    ),
    (
        "dataset.py",
        include_bytes!("../../resources/analysis_worker/dataset.py"),
    ),
    (
        "features.py",
        include_bytes!("../../resources/analysis_worker/features.py"),
    ),
    (
        "classification.py",
        include_bytes!("../../resources/analysis_worker/classification.py"),
    ),
    (
        "profiles.py",
        include_bytes!("../../resources/analysis_worker/profiles.py"),
    ),
    (
        "clustering.py",
        include_bytes!("../../resources/analysis_worker/clustering.py"),
    ),
    (
        "reduction.py",
        include_bytes!("../../resources/analysis_worker/reduction.py"),
    ),
];

fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
fn remote_command(settings: &Settings, args: &[String]) -> Result<Command, String> {
    let target = if settings.analysis_ssh_target.is_empty() {
        &settings.ssh_target
    } else {
        &settings.analysis_ssh_target
    };
    if !crate::store::valid_ssh_target(target) {
        return Err("分析 SSH 目标无效".into());
    }
    let mut c = Command::new("ssh");
    c.args([
        "-o",
        "BatchMode=yes",
        "-o",
        "ConnectTimeout=15",
        "-o",
        "ServerAliveInterval=10",
        "-o",
        "ServerAliveCountMax=2",
        target,
    ])
    .arg(args.iter().map(|s| quote(s)).collect::<Vec<_>>().join(" "));
    Ok(c)
}
fn tail(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_default()
        .chars()
        .rev()
        .take(1600)
        .collect::<String>()
        .chars()
        .rev()
        .collect()
}
fn wait(mut child: std::process::Child, cancel: &AtomicBool, log: &Path) -> Result<(), String> {
    loop {
        if cancel.load(Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("已中断".into());
        }
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(s) if s.success() => return Ok(()),
            Some(s) => return Err(format!("分析运行失败 ({s})：{}", tail(log))),
            None => std::thread::sleep(Duration::from_millis(80)),
        }
    }
}
fn events(
    mut command: Command,
    state: &AppState,
    app: &AppHandle,
    job: &Job,
    cancel: &AtomicBool,
    log: &Path,
) -> Result<Value, String> {
    if cancel.load(Ordering::SeqCst) {
        return Err("已中断".into());
    }
    command
        .stdout(Stdio::piped())
        .stdin(Stdio::piped())
        .stderr(Stdio::from(File::create(log).map_err(|e| e.to_string())?));
    let mut child = command
        .spawn()
        .map_err(|e| format!("启动分析工作进程失败：{e}；请检查分析环境设置"))?;
    let stdout = child.stdout.take().ok_or("分析输出管道缺失")?;
    let (tx, rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let value = line.map_err(|e| e.to_string()).and_then(|s| {
                serde_json::from_str::<Value>(&s).map_err(|e| format!("分析协议错误：{e}"))
            });
            if tx.send(value).is_err() {
                break;
            }
        }
    });
    let mut result = None;
    let mut error = None;
    loop {
        while let Ok(value) = rx.try_recv() {
            consume(value, state, app, job, &mut result, &mut error);
        }
        if cancel.load(Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            drop(rx);
            let _ = reader.join();
            return Err("已中断".into());
        }
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            let _ = reader.join();
            while let Ok(value) = rx.try_recv() {
                consume(value, state, app, job, &mut result, &mut error);
            }
            if let Some(e) = error {
                return Err(e);
            }
            if !status.success() {
                return Err(format!("分析工作进程失败：{}", tail(log)));
            }
            return result.ok_or_else(|| format!("分析没有生成可登记结果：{}", tail(log)));
        }
        std::thread::sleep(Duration::from_millis(80));
    }
}
fn consume(
    value: Result<Value, String>,
    state: &AppState,
    app: &AppHandle,
    job: &Job,
    result: &mut Option<Value>,
    error: &mut Option<String>,
) {
    match value {
        Ok(v) => match v["event"].as_str() {
            Some("progress") => {
                let _ = state.update_job(
                    app,
                    &job.id,
                    "running",
                    v["stage"].as_str().unwrap_or("分析中"),
                    None,
                    None,
                );
            }
            Some("result") => *result = Some(v["result"].clone()),
            Some("error") => {
                *error = Some(format!(
                    "{}：{}",
                    v["code"].as_str().unwrap_or("analysis_failed"),
                    v["message"].as_str().unwrap_or("分析失败")
                ))
            }
            _ => *error = Some("未知分析协议事件".into()),
        },
        Err(e) => *error = Some(e),
    }
}
fn deploy(root: &Path) -> Result<PathBuf, String> {
    let package = root.join("analysis").join("worker").join("analysis_worker");
    fs::create_dir_all(&package).map_err(|e| e.to_string())?;
    for (name, bytes) in SOURCES {
        fs::write(package.join(name), bytes).map_err(|e| e.to_string())?;
    }
    Ok(package.parent().ok_or("工作进程路径无效")?.to_path_buf())
}
fn helper(
    settings: &Settings,
    args: Vec<String>,
    input: Option<&Path>,
    output: &Path,
    cancel: &AtomicBool,
    log: &Path,
) -> Result<(), String> {
    let mut c = remote_command(settings, &args)?;
    c.stdout(Stdio::from(
        File::create(output).map_err(|e| e.to_string())?,
    ))
    .stderr(Stdio::from(File::create(log).map_err(|e| e.to_string())?));
    if let Some(p) = input {
        c.stdin(Stdio::from(File::open(p).map_err(|e| e.to_string())?));
    } else {
        c.stdin(Stdio::null());
    }
    wait(c.spawn().map_err(|e| e.to_string())?, cancel, log)
}
fn transfer(
    settings: &Settings,
    python: &str,
    local: &Path,
    remote: &str,
    upload: bool,
    work: &Path,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let code = if upload {
        "import sys;from pathlib import Path;p=Path(sys.argv[1]);p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(sys.stdin.buffer.read())"
    } else {
        "import sys,shutil;from pathlib import Path;f=Path(sys.argv[1]).open('rb');shutil.copyfileobj(f,sys.stdout.buffer)"
    };
    let output = work.join("transfer.out");
    helper(
        settings,
        vec![python.into(), "-c".into(), code.into(), remote.into()],
        if upload { Some(local) } else { None },
        if upload { &output } else { local },
        cancel,
        &work.join("transfer.log"),
    )
}
fn safe_name(name: &str) -> bool {
    !name.starts_with('.')
        && !name.is_empty()
        && name.len() < 128
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

pub fn execute(
    state: &AppState,
    app: &AppHandle,
    job: &Job,
    settings: &Settings,
    request: &Value,
    cancel: Arc<AtomicBool>,
) -> Result<Value, String> {
    let worker = deploy(&state.root)?;
    let work = state.root.join("jobs").join(&job.id);
    fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    if settings.analysis_mode == "ssh" {
        return remote(state, app, job, settings, request, cancel, &work, &worker);
    }
    let input = work.join("analysis-request.json");
    fs::write(
        &input,
        serde_json::to_vec(request).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let executable = if cfg!(windows) {
        "analysis-worker.exe"
    } else {
        "analysis-worker"
    };
    let bundled = app
        .path()
        .resource_dir()
        .map_err(|e| e.to_string())?
        .join("resources/runtime/analysis-worker")
        .join(executable);
    let mut command = if settings.analysis_python_bin.is_empty() && bundled.is_file() {
        let mut c = Command::new(bundled);
        c.arg(&input);
        c
    } else {
        let mut c = Command::new(if settings.analysis_python_bin.is_empty() {
            &settings.python_bin
        } else {
            &settings.analysis_python_bin
        });
        c.args(["-m", "analysis_worker"]).arg(&input);
        c
    };
    command
        .current_dir(&worker)
        .env("PYTHONPATH", &worker)
        .env("ML_WATCH_STDIN", "1")
        .env("PYTHONUTF8", "1");
    for k in ["OMP_NUM_THREADS", "OPENBLAS_NUM_THREADS", "MKL_NUM_THREADS"] {
        command.env(k, "2");
    }
    events(
        command,
        state,
        app,
        job,
        &cancel,
        &work.join("analysis.log"),
    )
}
fn remote(
    state: &AppState,
    app: &AppHandle,
    job: &Job,
    settings: &Settings,
    request: &Value,
    cancel: Arc<AtomicBool>,
    work: &Path,
    worker: &Path,
) -> Result<Value, String> {
    let python = if settings.analysis_remote_python_bin.is_empty() {
        &settings.remote_python_bin
    } else {
        &settings.analysis_remote_python_bin
    };
    let output = work.join("remote-control.out");
    let log = work.join("remote-control.log");
    helper(
        settings,
        vec![
            python.clone(),
            "-c".into(),
            "import tempfile;print(tempfile.mkdtemp(prefix='ml-analysis-'))".into(),
        ],
        None,
        &output,
        &cancel,
        &log,
    )?;
    let remote_root = fs::read_to_string(&output)
        .map_err(|e| e.to_string())?
        .trim()
        .to_owned();
    if remote_root.is_empty() || remote_root.contains('\n') {
        return Err("远端分析临时目录无效".into());
    }
    let outcome = (|| -> Result<Value, String> {
        for (name, _) in SOURCES {
            transfer(
                settings,
                python,
                &worker.join("analysis_worker").join(name),
                &format!("{remote_root}/analysis_worker/{name}"),
                true,
                work,
                &cancel,
            )?;
        }
        let mut req = request.clone();
        req["dataset_dir"] = json!(format!("{remote_root}/dataset"));
        req["assets_dir"] = json!(format!("{remote_root}/assets"));
        let action = request["action"].as_str().unwrap_or("");
        if action == "prepare" {
            for name in ["users.dat", "movies.dat", "ratings.dat"] {
                let source = request["dataset"]["files"][name]
                    .as_str()
                    .ok_or("清洗输入缺失")?;
                let path = format!("{remote_root}/input/{name}");
                transfer(
                    settings,
                    python,
                    Path::new(source),
                    &path,
                    true,
                    work,
                    &cancel,
                )?;
                req["dataset"]["files"][name] = json!(path);
            }
        } else if action != "check" {
            let dataset = request["dataset_dir"].as_str().ok_or("数据集路径缺失")?;
            for name in ["manifest.json", "entities.json", "ratings.npy"] {
                transfer(
                    settings,
                    python,
                    &Path::new(dataset).join(name),
                    &format!("{remote_root}/dataset/{name}"),
                    true,
                    work,
                    &cancel,
                )?;
            }
            let local_assets = Path::new(request["assets_dir"].as_str().ok_or("资产目录缺失")?);
            if local_assets.exists() {
                for entry in fs::read_dir(local_assets)
                    .map_err(|e| e.to_string())?
                    .flatten()
                {
                    let path = entry.path();
                    let Ok(bytes) = fs::read(path.join("metadata.json")) else {
                        continue;
                    };
                    let Ok(meta) = serde_json::from_slice::<Value>(&bytes) else {
                        continue;
                    };
                    if meta["dataset_id"] != request["dataset_id"]
                        || meta["task"] != request["action"]
                    {
                        continue;
                    }
                    let id = meta["id"].as_str().ok_or("资产 ID 缺失")?;
                    uuid::Uuid::parse_str(id).map_err(|_| "资产 ID 无效")?;
                    for name in ["metadata.json", "result.json", "model.pkl"] {
                        if path.join(name).is_file() {
                            transfer(
                                settings,
                                python,
                                &path.join(name),
                                &format!("{remote_root}/assets/{id}/{name}"),
                                true,
                                work,
                                &cancel,
                            )?;
                        }
                    }
                }
            }
        }
        let input = work.join("remote-request.json");
        fs::write(&input, serde_json::to_vec(&req).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        let remote_input = format!("{remote_root}/request.json");
        transfer(settings, python, &input, &remote_input, true, work, &cancel)?;
        let script="import os,sys,runpy;os.chdir(sys.argv[1]);os.environ.update(ML_WATCH_STDIN='1',OMP_NUM_THREADS='2',OPENBLAS_NUM_THREADS='2',MKL_NUM_THREADS='2');sys.argv=['analysis_worker',sys.argv[2]];runpy.run_module('analysis_worker',run_name='__main__')";
        let command = remote_command(
            settings,
            &[
                python.clone(),
                "-c".into(),
                script.into(),
                remote_root.clone(),
                remote_input,
            ],
        )?;
        let result = events(
            command,
            state,
            app,
            job,
            &cancel,
            &work.join("analysis.log"),
        )?;
        if action == "check" {
            return Ok(result);
        }
        let (remote_folder, local_folder) = if action == "prepare" {
            (
                format!("{remote_root}/dataset"),
                PathBuf::from(request["dataset_dir"].as_str().ok_or("数据集路径缺失")?),
            )
        } else {
            let id = result["asset_id"].as_str().ok_or("结果没有资产 ID")?;
            uuid::Uuid::parse_str(id).map_err(|_| "资产 ID 无效")?;
            (
                format!("{remote_root}/assets/{id}"),
                PathBuf::from(request["assets_dir"].as_str().ok_or("资产目录缺失")?).join(id),
            )
        };
        fs::create_dir_all(&local_folder).map_err(|e| e.to_string())?;
        helper(settings,vec![python.clone(),"-c".into(),"import json,sys;from pathlib import Path;print(json.dumps([p.name for p in Path(sys.argv[1]).iterdir() if p.is_file() and not p.name.endswith('.dat')]))".into(),remote_folder.clone()],None,&output,&cancel,&log)?;
        let names: Vec<String> =
            serde_json::from_slice(&fs::read(&output).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        // Publish marker is downloaded last; interrupted transfers remain unavailable.
        let mut names = names;
        names.sort_by_key(|name| name == "metadata.json" || name == "manifest.json");
        for name in names {
            if !safe_name(&name) {
                return Err("远端产物文件名无效".into());
            }
            transfer(
                settings,
                python,
                &local_folder.join(&name),
                &format!("{remote_folder}/{name}"),
                false,
                work,
                &cancel,
            )?;
        }
        Ok(result)
    })();
    let cleanup_cancel = AtomicBool::new(false);
    let _ = helper(
        settings,
        vec![
            python.clone(),
            "-c".into(),
            "import shutil,sys;shutil.rmtree(sys.argv[1],ignore_errors=True)".into(),
            remote_root,
        ],
        None,
        &output,
        &cleanup_cancel,
        &log,
    );
    outcome
}

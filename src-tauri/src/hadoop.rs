use crate::{model::{Job, Rules, Settings, UploadedFile}, store::AppState};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{fs::{self, File}, io::{BufRead, BufReader, BufWriter, Read, Write}, path::{Path, PathBuf},
    process::{Command, Stdio}, sync::{atomic::{AtomicBool, Ordering}, Arc}, time::Duration};
use tauri::AppHandle;

fn shell_quote(value: &str) -> String { format!("'{}'", value.replace('\'', "'\\''")) }

struct Runner<'a> { settings: &'a Settings, work: &'a Path, cancel: &'a AtomicBool,
    progress: Option<(&'a AppState, &'a AppHandle, &'a str)> }

impl Runner<'_> {
    fn cancel_job(&self, log: &Path) {
        let content = fs::read_to_string(log).unwrap_or_default();
        let job_id = content.split_whitespace().rev().map(|s| s.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_'))
            .find(|s| s.starts_with("job_") && s[4..].chars().all(|c| c.is_ascii_digit() || c == '_'));
        let Some(job_id) = job_id else { return; };
        let binary = if self.settings.hadoop_mode == "ssh" { &self.settings.remote_hadoop_bin }
            else { &self.settings.hadoop_bin };
        if let Ok(mut command) = self.command(&[binary.clone(), "job".into(), "-kill".into(), job_id.into()]) {
            command.stdout(Stdio::null()).stderr(Stdio::null());
            if let Ok(mut process) = command.spawn() {
                for _ in 0..50 {
                    if process.try_wait().ok().flatten().is_some() { return; }
                    std::thread::sleep(Duration::from_millis(100));
                }
                let _ = process.kill(); let _ = process.wait();
            }
        }
    }

    fn command(&self, args: &[String]) -> Result<Command, String> {
        if self.settings.hadoop_mode == "ssh" {
            if !crate::store::valid_ssh_target(&self.settings.ssh_target) { return Err("SSH 目标无效".into()); }
            let mut command = Command::new("ssh");
            command.arg("-o").arg("BatchMode=yes").arg(&self.settings.ssh_target)
                .arg(args.iter().map(|s| shell_quote(s)).collect::<Vec<_>>().join(" "));
            Ok(command)
        } else {
            let mut command = Command::new(&args[0]);
            if !self.settings.hadoop_conf_dir.is_empty() {
                command.env("HADOOP_CONF_DIR", &self.settings.hadoop_conf_dir);
            }
            command.args(&args[1..]); Ok(command)
        }
    }

    fn run(&self, label: &str, args: Vec<String>, input: Option<&Path>, output: Option<&Path>) -> Result<(), String> {
        if self.cancel.load(Ordering::SeqCst) { return Err("已中断".into()); }
        let mut command = self.command(&args)?;
        let log = self.work.join(format!("{label}.log"));
        command.stderr(Stdio::from(File::create(&log).map_err(|e| e.to_string())?));
        command.stdout(Stdio::from(File::create(output.unwrap_or(&log.with_extension("out")))
            .map_err(|e| e.to_string())?));
        if let Some(path) = input { command.stdin(Stdio::from(File::open(path).map_err(|e| e.to_string())?)); }
        let mut child = command.spawn().map_err(|e| format!("启动 {label} 失败: {e}"))?;
        let mut last_progress = std::time::Instant::now();
        let mut previous_stage = String::new();
        loop {
            if label == "streaming" && last_progress.elapsed() >= Duration::from_secs(1) {
                if let Some((state, app, id)) = self.progress {
                    if let Some(stage) = streaming_progress(&log) {
                        if stage != previous_stage {
                            let _ = state.update_job(app, id, "running", &stage, None, None);
                            previous_stage = stage;
                        }
                    }
                }
                last_progress = std::time::Instant::now();
            }
            if self.cancel.load(Ordering::SeqCst) {
                let _ = child.kill(); let _ = child.wait();
                if label == "streaming" { self.cancel_job(&log); }
                return Err("已中断".into());
            }
            match child.try_wait() {
                Ok(Some(status)) if status.success() => return Ok(()),
                Ok(Some(status)) => {
                    let detail = fs::read_to_string(&log).unwrap_or_default();
                    return Err(format!("{label} 失败 ({status}): {}", detail.chars().rev().take(1600).collect::<String>().chars().rev().collect::<String>()));
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(150)),
                Err(e) => return Err(e.to_string()),
            }
        }
    }

    fn hadoop(&self, label: &str, args: Vec<String>, output: Option<&Path>) -> Result<(), String> {
        let binary = if self.settings.hadoop_mode == "ssh" { &self.settings.remote_hadoop_bin }
            else { &self.settings.hadoop_bin };
        self.run(label, std::iter::once(binary.clone()).chain(args).collect(), None, output)
    }

    fn upload_remote(&self, local: &Path, remote: &str, label: &str) -> Result<(), String> {
        if self.cancel.load(Ordering::SeqCst) { return Err("已中断".into()); }
        let mut child = Command::new("ssh").arg("-o").arg("BatchMode=yes")
            .arg(&self.settings.ssh_target).arg(format!("cat > {}", shell_quote(remote)))
            .stdin(Stdio::from(File::open(local).map_err(|e| e.to_string())?))
            .stderr(Stdio::from(File::create(self.work.join(format!("{label}.log"))).map_err(|e| e.to_string())?))
            .spawn().map_err(|e| e.to_string())?;
        loop {
            if self.cancel.load(Ordering::SeqCst) { let _ = child.kill(); let _ = child.wait(); return Err("已中断".into()); }
            match child.try_wait() {
                Ok(Some(status)) if status.success() => return Ok(()),
                Ok(Some(status)) => return Err(format!("远端文件上传失败: {status}")),
                Ok(None) => std::thread::sleep(Duration::from_millis(150)),
                Err(e) => return Err(e.to_string()),
            }
        }
    }

    fn put_generated(&self, local: &Path, remote_dir: &str, hdfs_path: &str, label: &str) -> Result<(), String> {
        let source = if self.settings.hadoop_mode == "ssh" {
            let remote = format!("{remote_dir}/{}", local.file_name().ok_or("文件名缺失")?.to_string_lossy());
            self.upload_remote(local, &remote, &format!("copy-{label}"))?;
            remote
        } else { local.to_string_lossy().into() };
        self.hadoop(label, vec!["fs".into(), "-put".into(), "-f".into(), source, hdfs_path.into()], None)
    }
}

fn streaming_progress(log: &Path) -> Option<String> {
    let text = fs::read_to_string(log).ok()?;
    for line in text.lines().rev() {
        if let Some(index) = line.find(" map ") {
            let rest = &line[index + 5..];
            if let Some((map, reduce)) = rest.split_once(" reduce ") {
                let map = map.split_whitespace().next()?;
                let reduce = reduce.split_whitespace().next()?;
                if map.ends_with('%') && reduce.ends_with('%') {
                    return Some(format!("Hadoop Map {map} · Reduce {reduce}"));
                }
            }
        }
    }
    None
}

fn jar_path(settings: &Settings) -> Result<String, String> {
    if !settings.streaming_jar.trim().is_empty() { return Ok(settings.streaming_jar.clone()); }
    if settings.hadoop_mode == "ssh" { return Err("SSH 模式请配置远端 Hadoop Streaming jar 路径".into()); }
    let home = std::env::var_os("HADOOP_HOME").map(PathBuf::from).or_else(|| {
        let path = Path::new(&settings.hadoop_bin);
        if path.is_absolute() { path.parent()?.parent().map(Path::to_path_buf) } else { None }
    }).ok_or("请设置 HADOOP_HOME 或在设置中指定 Streaming jar")?;
    let dir = home.join("share/hadoop/tools/lib");
    let entry = fs::read_dir(&dir).map_err(|e| format!("查找 Streaming jar 失败: {e}"))?
        .filter_map(Result::ok).find(|e| e.file_name().to_string_lossy().starts_with("hadoop-streaming-")
            && e.path().extension().is_some_and(|x| x == "jar"))
        .ok_or("未找到 Hadoop Streaming jar")?;
    Ok(entry.path().to_string_lossy().into())
}

fn work_dir(state: &AppState, id: &str) -> Result<PathBuf, String> {
    let dir = state.root.join("jobs").join(id);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn stream_job(runner: &Runner<'_>, settings: &Settings, work: &Path, inputs: &[String],
    output: &str, rules: &Rules, mode: &str, utf8_input: bool, reference_override: Option<i64>) -> Result<Value, String> {
    let script = work.join("govern.py"); let rule_file = work.join("rules.json");
    fs::write(&script, include_bytes!("../resources/govern.py")).map_err(|e| e.to_string())?;
    let mut rule_doc = serde_json::to_value(rules).map_err(|e| e.to_string())?;
    rule_doc["assessment_reference"] = json!(reference_override);
    fs::write(&rule_file, serde_json::to_vec(&rule_doc).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let (script_source, rules_source) = if settings.hadoop_mode == "ssh" {
        let job_id = work.file_name().and_then(|name| name.to_str()).ok_or("任务目录无效")?;
        uuid::Uuid::parse_str(job_id).map_err(|_| "任务 ID 无效")?;
        let remote_dir = format!("/tmp/ml1m-agent-{job_id}");
        runner.run("mkdir-remote", vec!["mkdir".into(), "-p".into(), remote_dir.clone()], None, None)?;
        let a = format!("{remote_dir}/govern.py"); let b = format!("{remote_dir}/rules.json");
        runner.upload_remote(&script, &a, "copy-script")?;
        runner.upload_remote(&rule_file, &b, "copy-rules")?; (a, b)
    } else { (script.to_string_lossy().into(), rule_file.to_string_lossy().into()) };
    let python = if settings.hadoop_mode == "ssh" { &settings.remote_python_bin } else { &settings.python_bin };
    let mut args = vec!["jar".into(), jar_path(settings)?, "-D".into(), "mapreduce.job.reduces=1".into(),
        "-files".into(), format!("{script_source}#govern.py,{rules_source}#rules.json")];
    if utf8_input { args.extend(["-cmdenv".into(), "ML_INPUT_ENCODING=utf-8".into()]); }
    for input in inputs { args.extend(["-input".into(), input.clone()]); }
    args.extend(["-output".into(), output.into(), "-mapper".into(), format!("{python} govern.py map"),
        "-reducer".into(), format!("{python} govern.py {mode}")]);
    runner.hadoop("streaming", args, None)?;
    let output_file = work.join("stream-output.txt");
    runner.hadoop("get-output", vec!["fs".into(), "-cat".into(), format!("{output}/part-*")], Some(&output_file))?;
    let mut metrics = None;
    let mut writers = std::collections::HashMap::new();
    if mode == "clean" {
        for (kind, name) in [("0", "users.dat"), ("1", "movies.dat"), ("2", "ratings.dat")] {
            writers.insert(kind, BufWriter::new(File::create(work.join(name)).map_err(|e| e.to_string())?));
        }
    }
    let mut actions = BufWriter::new(File::create(work.join("actions.ndjson")).map_err(|e| e.to_string())?);
    for line in BufReader::new(File::open(&output_file).map_err(|e| e.to_string())?).lines() {
        if runner.cancel.load(Ordering::SeqCst) { return Err("已中断".into()); }
        let line = line.map_err(|e| e.to_string())?;
        let (tag, body) = line.split_once('\t').ok_or("Hadoop 输出格式错误")?;
        let value: Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
        match tag {
            "C" => { let kind = value["table"].as_str().ok_or("表名缺失")?;
                writeln!(writers.get_mut(kind).ok_or("输出表无效")?, "{}", value["line"].as_str().ok_or("记录缺失")?)
                    .map_err(|e| e.to_string())?; },
            "A" => writeln!(actions, "{body}").map_err(|e| e.to_string())?,
            "M" => metrics = Some(value),
            _ => return Err("Hadoop 输出标签无效".into()),
        }
    }
    for (_, mut writer) in writers { writer.flush().map_err(|e| e.to_string())?; }
    actions.flush().map_err(|e| e.to_string())?;
    metrics.ok_or("Hadoop 未返回任务结果".into())
}

pub fn initialize_file(state: &AppState, file: &UploadedFile) -> Result<UploadedFile, String> {
    let db = state.snapshot()?;
    let settings = db.settings;
    let cancel = AtomicBool::new(false);
    let work = work_dir(state, &file.id)?;
    let runner = Runner { settings: &settings, work: &work, cancel: &cancel, progress: None };
    let parent = format!("{}/sources/{}", settings.hdfs_root.trim_end_matches('/'), file.id);
    let destination = format!("{parent}/{}", file.name);
    runner.hadoop("mkdir-source", vec!["fs".into(), "-mkdir".into(), "-p".into(), parent], None)?;
    let source = if settings.hadoop_mode == "ssh" {
        let remote_dir = format!("/tmp/ml1m-agent-{}", file.id);
        runner.run("mkdir-remote", vec!["mkdir".into(), "-p".into(), remote_dir.clone()], None, None)?;
        let remote = format!("{remote_dir}/{}", file.name);
        runner.upload_remote(Path::new(&file.local_path), &remote, "copy-source")?; remote
    } else { file.local_path.clone() };
    runner.hadoop("put-source", vec!["fs".into(), "-put".into(), "-f".into(), source, destination.clone()], None)?;
    state.change(|db| {
        let saved = db.files.iter_mut().find(|item| item.id == file.id).ok_or("文件记录不存在")?;
        saved.hdfs_path = Some(destination);
        let updated = saved.clone();
        db.active_files.insert(file.name.clone(), file.id.clone());
        if ["users.dat", "movies.dat", "ratings.dat"].iter().all(|name| db.active_files.contains_key(*name)) { db.source_revision += 1; }
        Ok(updated)
    })
}

pub fn source_status(state: &AppState) -> Result<Value, String> {
    let db = state.snapshot()?;
    let cancel = AtomicBool::new(false);
    let work = state.root.join("transfer-logs").join("status");
    fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let runner = Runner { settings: &db.settings, work: &work, cancel: &cancel, progress: None };
    let mut out = serde_json::Map::new();
    for name in ["users.dat", "movies.dat", "ratings.dat"] {
        let file = db.active_files.get(name).and_then(|id| db.files.iter().find(|f| &f.id == id));
        let item = match file {
            Some(file) => {
                let checked = file.hdfs_path.as_ref().map(|path| runner.hadoop(
                    &format!("test-{name}"), vec!["fs".into(), "-test".into(), "-e".into(), path.clone()], None));
                let exists = checked.as_ref().is_some_and(Result::is_ok);
                let error = checked.and_then(Result::err);
                json!({"exists":exists,"file":file,"hdfs_path":file.hdfs_path,"error":error})
            },
            None => json!({"exists":false,"file":null,"hdfs_path":null}),
        };
        out.insert(name.into(), item);
    }
    Ok(Value::Object(out))
}

fn job_location(settings: &Settings, root: &str) -> Value {
    json!({"hdfs_job_root":root,"hadoop_connection":{
        "hadoop_mode":settings.hadoop_mode,"ssh_target":settings.ssh_target,
        "hadoop_bin":settings.hadoop_bin,"remote_hadoop_bin":settings.remote_hadoop_bin,
        "hadoop_conf_dir":settings.hadoop_conf_dir
    }})
}

pub fn delete_job_results(state: &AppState, settings: &Settings, jobs: &[Job]) -> Result<(), String> {
    // Validate every stored path before running any destructive command.
    let mut targets = Vec::new();
    for job in jobs {
        uuid::Uuid::parse_str(&job.id).map_err(|_| format!("任务 ID 无效: {}", job.id))?;
        if job.kind != "cleaning" && job.kind != "assessment" { continue; }
        let result = job.result.as_ref();
        let root = result.and_then(|r| r["hdfs_job_root"].as_str())
            .or_else(|| result.and_then(|r| r["clean_hdfs"].as_str()).and_then(|p| p.strip_suffix("/clean")))
            .map(str::to_owned)
            .unwrap_or_else(|| format!("{}/jobs/{}", settings.hdfs_root.trim_end_matches('/'), job.id));
        if !root.starts_with('/') || root.contains("..") || root.contains("//")
            || !root.ends_with(&format!("/jobs/{}", job.id)) {
            return Err(format!("任务 {} 的 HDFS 目录无效", job.id));
        }
        let mut connection = settings.clone();
        if let Some(saved) = result.and_then(|r| r.get("hadoop_connection")) {
            for (field, target) in [
                ("hadoop_mode", &mut connection.hadoop_mode),
                ("ssh_target", &mut connection.ssh_target),
                ("hadoop_bin", &mut connection.hadoop_bin),
                ("remote_hadoop_bin", &mut connection.remote_hadoop_bin),
                ("hadoop_conf_dir", &mut connection.hadoop_conf_dir),
            ] {
                *target = saved[field].as_str().ok_or_else(|| format!("任务 {} 缺少 Hadoop 连接信息: {field}", job.id))?.into();
            }
        }
        if !["local", "ssh"].contains(&connection.hadoop_mode.as_str())
            || (connection.hadoop_mode == "ssh" && !crate::store::valid_ssh_target(&connection.ssh_target)) {
            return Err(format!("任务 {} 的 Hadoop 连接信息无效", job.id));
        }
        targets.push((job.id.clone(), root, connection));
    }
    let cancel = AtomicBool::new(false);
    for (id, root, connection) in targets {
        let work = state.root.join("jobs").join(&id);
        fs::create_dir_all(&work).map_err(|e| e.to_string())?;
        let runner = Runner { settings: &connection, work: &work, cancel: &cancel, progress: None };
        runner.hadoop("delete-hdfs", vec!["fs".into(), "-rm".into(), "-r".into(), "-f".into(), "-skipTrash".into(), root], None)
            .map_err(|e| format!("删除任务 {id} 的 HDFS 数据失败: {e}"))?;
        if connection.hadoop_mode == "ssh" {
            runner.run("delete-remote", vec!["rm".into(), "-rf".into(), format!("/tmp/ml1m-agent-{id}")], None, None)
                .map_err(|e| format!("删除任务 {id} 的远端临时文件失败: {e}"))?;
        }
    }
    Ok(())
}

pub fn run_cleaning(state: &AppState, app: &AppHandle, job: &Job, settings: &Settings,
    files: &[UploadedFile], rules: &Rules, cancel: Arc<AtomicBool>) -> Result<Value, String> {
    let db = state.snapshot()?;
    let rules_revision = db.conversations.iter().find(|c| c.id == job.conversation_id).ok_or("对话不存在")?.rules_revision;
    let work = work_dir(state, &job.id)?;
    let runner = Runner { settings, work: &work, cancel: &cancel, progress: Some((state, app, &job.id)) };
    let inputs = files.iter().map(|file| file.hdfs_path.clone().ok_or(format!("{} 尚未初始化到 HDFS", file.name))).collect::<Result<Vec<_>, _>>()?;
    let root = format!("{}/jobs/{}", settings.hdfs_root.trim_end_matches('/'), job.id);
    let location = job_location(settings, &root);
    state.update_job(app, &job.id, "running", "Hadoop 正在清洗", None, Some(location.clone()))?;
    runner.hadoop("mkdir-job", vec!["fs".into(), "-mkdir".into(), "-p".into(), root.clone()], None)?;
    let metrics = stream_job(&runner, settings, &work, &inputs, &format!("{root}/stream-output"), rules, "clean", false, None)?;
    state.update_job(app, &job.id, "running", "上传清洗结果", None, None)?;
    let clean_root = format!("{root}/clean");
    runner.hadoop("mkdir-clean", vec!["fs".into(), "-mkdir".into(), "-p".into(), clean_root.clone()], None)?;
    let remote_dir = format!("/tmp/ml1m-agent-{}", job.id);
    if settings.hadoop_mode == "ssh" { runner.run("mkdir-result-remote", vec!["mkdir".into(), "-p".into(), remote_dir.clone()], None, None)?; }
    for name in ["users.dat", "movies.dat", "ratings.dat", "actions.ndjson"] {
        runner.put_generated(&work.join(name), &remote_dir, &format!("{clean_root}/{name}"), &format!("put-{name}"))?;
    }
    let mut source_hash = Sha256::new();
    for name in ["users.dat", "movies.dat", "ratings.dat"] {
        let file = files.iter().find(|f| f.name == name).ok_or("数据源缺失")?;
        source_hash.update(name.as_bytes()); source_hash.update(file.sha256.as_bytes());
    }
    let mut clean_hash = Sha256::new();
    for name in ["users.dat", "movies.dat", "ratings.dat"] {
        clean_hash.update(name.as_bytes());
        let mut file = File::open(work.join(name)).map_err(|e| e.to_string())?;
        let mut bytes = [0_u8; 65536];
        loop { let count = file.read(&mut bytes).map_err(|e| e.to_string())?;
            if count == 0 { break; } clean_hash.update(&bytes[..count]); }
    }
    let result = json!({"job_id":job.id,"kind":"cleaning","clean_hdfs":clean_root,
        "hdfs_job_root":root,"hadoop_connection":location["hadoop_connection"],
        "source_files":files.iter().map(|f| json!({"name":f.name,"sha256":f.sha256,"hdfs_path":f.hdfs_path})).collect::<Vec<_>>(),
        "data_version":format!("sha256:{:x}",source_hash.finalize()),
        "data_version_label":format!("原始数据 v{}",db.source_revision),
        "clean_data_version":format!("sha256:{:x}",clean_hash.finalize()),
        "clean_data_version_label":format!("清洗数据 v{}",job.display_number),
        "rule_version":crate::rules::version(rules),
        "rule_version_label":format!("规则方案 v{rules_revision}"),
        "algorithm_version":format!("sha256:{:x}",Sha256::digest(include_bytes!("../resources/govern.py"))),
        "algorithm_version_label":format!("清洗算法 v{}",env!("CARGO_PKG_VERSION")),
        "rules":rules,"metrics":metrics,
        "files":["users.dat","movies.dat","ratings.dat","actions.ndjson"]});
    write_report_json(&work, &result)?;
    runner.put_generated(&work.join("report.json"), &remote_dir, &format!("{clean_root}/report.json"), "put-report")?;
    Ok(result)
}

pub fn run_assessment(state: &AppState, app: &AppHandle, job: &Job, settings: &Settings,
    inputs: &[String], rules: &Rules, source: Value, utf8_input: bool,
    cancel: Arc<AtomicBool>, reference_override: Option<i64>) -> Result<Value, String> {
    let db = state.snapshot()?;
    let rules_revision = db.conversations.iter().find(|c| c.id == job.conversation_id).ok_or("对话不存在")?.rules_revision;
    let work = work_dir(state, &job.id)?;
    let runner = Runner { settings, work: &work, cancel: &cancel, progress: Some((state, app, &job.id)) };
    let root = format!("{}/jobs/{}", settings.hdfs_root.trim_end_matches('/'), job.id);
    let location = job_location(settings, &root);
    state.update_job(app, &job.id, "running", "Hadoop 正在计算五维评估", None, Some(location.clone()))?;
    runner.hadoop("mkdir-job", vec!["fs".into(), "-mkdir".into(), "-p".into(), root.clone()], None)?;
    let output = format!("{root}/stream-output");
    let metrics = stream_job(&runner, settings, &work, inputs, &output, rules, "assess", utf8_input, reference_override)?;
    let data_version = source.get("data_version").or_else(|| source.get("clean_data_version")).cloned();
    let result = json!({"job_id":job.id,"kind":"assessment","source":source,"data_version":data_version,
        "hdfs_job_root":root,"hadoop_connection":location["hadoop_connection"],
        "data_version_label":source.get("data_version_label").or_else(|| source.get("clean_data_version_label")),
        "rule_version":crate::rules::version(rules),
        "rule_version_label":format!("规则方案 v{rules_revision}"),
        "algorithm_version":format!("sha256:{:x}",Sha256::digest(include_bytes!("../resources/govern.py"))),
        "algorithm_version_label":format!("评分算法 v{}",env!("CARGO_PKG_VERSION")),
        "rules":rules,"metrics":metrics,"files":[]});
    write_report_json(&work, &result)?;
    let remote_dir = format!("/tmp/ml1m-agent-{}", job.id);
    if settings.hadoop_mode == "ssh" {
        runner.run("mkdir-report-remote", vec!["mkdir".into(), "-p".into(), remote_dir.clone()], None, None)?;
    }
    runner.put_generated(&work.join("report.json"), &remote_dir, &format!("{root}/report.json"), "put-report")?;
    Ok(result)
}

pub fn compare(state: &AppState, first: &Job, second: &Job, job: &Job) -> Result<Value, String> {
    let a = first.result.as_ref().ok_or("第一个评估没有结果")?;
    let b = second.result.as_ref().ok_or("第二个评估没有结果")?;
    if a["rule_version"] != b["rule_version"] { return Err("两次评估规则版本不同，无法直接对比".into()); }
    if a["algorithm_version"] != b["algorithm_version"] { return Err("两次评估算法版本不同，无法直接对比".into()); }
    if a["metrics"]["reference_timestamp"] != b["metrics"]["reference_timestamp"] {
        return Err("两次评估时效参照时间不同，无法直接对比".into());
    }
    let mut delta = serde_json::Map::new();
    for name in ["accurate", "complete", "unique", "consistent", "up_to_date"] {
        let score = |v: &Value| -> Option<f64> {
            if v["metrics"]["enabled"][name] == false { return None; }
            let m = &v["metrics"]["scores"];
            let good = m[format!("{name}_good")].as_f64().unwrap_or(0.0);
            let eligible = m[format!("{name}_eligible")].as_f64()?;
            (eligible > 0.0).then_some(good / eligible * 100.0)
        };
        let old = score(a); let new = score(b);
        delta.insert(name.into(), json!({"before":old,"after":new,"change_pp":old.zip(new).map(|(x,y)| y-x)}));
    }
    let rule_label = if a["rule_version_label"] == b["rule_version_label"] {
        a["rule_version_label"].as_str().unwrap_or("历史规则方案").to_string()
    } else {
        format!("相同规则内容（{} / {}）",
            a["rule_version_label"].as_str().unwrap_or("历史规则方案"),
            b["rule_version_label"].as_str().unwrap_or("历史规则方案"))
    };
    let result = json!({"job_id":job.id,"kind":"comparison","before_job_id":first.id,
        "after_job_id":second.id,"rule_version":a["rule_version"],"rules":a["rules"],
        "before_label":if first.display_number == 0 { "历史五维评估".to_string() } else { format!("五维评估 #{}",first.display_number) },
        "after_label":if second.display_number == 0 { "历史五维评估".to_string() } else { format!("五维评估 #{}",second.display_number) },
        "rule_version_label":rule_label,"algorithm_version_label":a["algorithm_version_label"],
        "reference_timestamp":a["metrics"]["reference_timestamp"],
        "dimensions":delta,"files":[]});
    write_report_json(&work_dir(state, &job.id)?, &result)?;
    Ok(result)
}

fn write_report_json(work: &Path, result: &Value) -> Result<(), String> {
    fs::write(work.join("report.json"), serde_json::to_vec_pretty(result).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

pub fn cleaning_records(state: &AppState, job: &Job, rule_id: Option<&str>, table: Option<&str>,
    action: Option<&str>, offset: usize, limit: usize) -> Result<Value, String> {
    if job.kind != "cleaning" || job.status != "completed" { return Err("清洗任务尚未完成".into()); }
    if limit == 0 || limit > 200 { return Err("每页最多 200 条".into()); }
    let input = BufReader::new(File::open(state.root.join("jobs").join(&job.id).join("actions.ndjson")).map_err(|e| e.to_string())?);
    let mut matches = 0; let mut records = Vec::new();
    for line in input.lines() {
        let value: Value = serde_json::from_str(&line.map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        if rule_id.is_some_and(|r| !value["rules"].as_array().is_some_and(|a| a.iter().any(|x| x == r))) ||
            table.is_some_and(|t| value["table"] != t) || action.is_some_and(|a| value["action"] != a) { continue; }
        if matches >= offset && records.len() < limit { records.push(value); }
        matches += 1;
    }
    Ok(json!({"total":matches,"offset":offset,"limit":limit,"records":records}))
}

pub fn preview(state: &AppState, job_id: &str, name: &str) -> Result<Vec<String>, String> {
    let db = state.snapshot()?;
    let job = db.jobs.iter().find(|j| j.id == job_id && j.status == "completed").ok_or("任务尚未完成")?;
    if !public_artifact(&job.kind, name) { return Err("此任务没有可预览的该文件".into()); }
    if job.kind == "report" {
        let body = fs::read_to_string(state.root.join("jobs").join(job_id).join(name)).map_err(|e| e.to_string())?;
        return Ok(crate::report::humanize_legacy_report(&body).lines().take(100).map(str::to_string).collect());
    }
    BufReader::new(File::open(state.root.join("jobs").join(job_id).join(name)).map_err(|e| e.to_string())?)
        .lines().take(100).collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

pub fn export(state: &AppState, job_id: &str, name: &str, destination: &str) -> Result<(), String> {
    let db = state.snapshot()?;
    let job = db.jobs.iter().find(|j| j.id == job_id && j.status == "completed").ok_or("任务尚未完成")?;
    if !public_artifact(&job.kind, name) { return Err("此任务没有可下载的该文件".into()); }
    if job.kind == "report" {
        let body = fs::read_to_string(state.root.join("jobs").join(job_id).join(name)).map_err(|e| e.to_string())?;
        fs::write(destination, crate::report::humanize_legacy_report(&body)).map_err(|e| e.to_string())?;
    } else {
        fs::copy(state.root.join("jobs").join(job_id).join(name), destination).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn public_artifact(kind: &str, name: &str) -> bool {
    match kind {
        "cleaning" => ["users.dat", "movies.dat", "ratings.dat", "actions.ndjson"].contains(&name),
        "report" => name == "report.md",
        _ => false,
    }
}

#[cfg(test)]
mod artifact_tests {
    use super::public_artifact;
    #[test]
    fn assessments_and_comparisons_cannot_export_internal_reports() {
        assert!(public_artifact("cleaning", "ratings.dat"));
        assert!(public_artifact("report", "report.md"));
        for kind in ["assessment", "comparison", "pipeline"] {
            assert!(!public_artifact(kind, "report.json"));
            assert!(!public_artifact(kind, "report.md"));
        }
        assert!(!public_artifact("cleaning", "report.json"));
    }
}

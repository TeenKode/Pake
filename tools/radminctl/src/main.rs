//! radminctl: Linux CLI for the community Wine/TAP Radmin VPN backend.
//! No proprietary Windows software is bundled and the VPN protocol is not reimplemented.
use std::{env,fs,io::{self,Read},net::Ipv4Addr,os::unix::fs::{FileTypeExt,PermissionsExt},path::{Path,PathBuf},process::{Command,ExitCode,Stdio},time::{SystemTime,UNIX_EPOCH}};

type Result<T> = std::result::Result<T,Box<dyn std::error::Error>>;
const TAP:&str="radminvpn0";

fn main()->ExitCode {
    match run() {Ok(())=>ExitCode::SUCCESS,Err(e)=>{eprintln!("radminctl: {e}");ExitCode::FAILURE}}
}
fn run()->Result<()> {
    let args:Vec<String>=env::args().skip(1).collect();
    let sub=args.first().map(String::as_str).unwrap_or("help");
    let tail=&args[args.len().min(1)..];
    match sub {
        "help"|"--help"|"-h"=>help(),
        "version"|"--version"=>println!("radminctl {}",env!("CARGO_PKG_VERSION")),
        "configure"=>configure(tail)?,
        "up"|"gui"|"web-ui"=>start(sub,tail)?,
        "down"=>stop()?,
        "status"=>status(tail)?,
        "doctor"|"diagnose"=>diagnose(tail)?,
        "lxc-check"=>lxc(tail)?,
        "selftest"=>selftest()?,
        "logs"=>logs(tail)?,
        "neighbors"=>{let rc=Command::new("ip").args(["neigh","show","dev",TAP]).status()?;if !rc.success(){return Err("ip neigh failed".into());}},
        "ping"=>ping(tail)?,
        _=>return Err(format!("Unknown command: {sub}").into())
    }
    Ok(())
}
fn help(){
    println!(r#"radminctl — Linux Radmin VPN CLI (Wine/TAP backend)
  configure /path/to/radmin-vpn-linux   Set backend path
  up [--installer FILE]                Start without GUI
  gui [--installer FILE]               Original GUI (requires display)
  web-ui [--installer FILE]            GUI via noVNC (SSH tunnel recommended)
  down | status [--json]               Stop / show VPN status
  doctor [--json]                      Inspect dependencies and VPN state
  diagnose bundle DIRECTORY            Save private diagnostic report
  diagnose bundle DIRECTORY --include-logs  Include sensitive raw logs
  lxc-check [--tap-test] [--json]      Proxmox LXC capability probe
  logs [--source service|bridge|driver|launcher|gui] [--lines N]
  neighbors | ping 26.x.x.x | selftest | version

Backend: https://github.com/baptisterajaut/radmin-vpn-linux
The join/leave network protocol is not implemented; use gui/web-ui for setup.
VNC requires RADMINCTL_VNC_PASSWORD (exactly 8 ASCII characters).
"#);
}
fn home()->Result<PathBuf>{Ok(PathBuf::from(env::var_os("HOME").ok_or("HOME not set")?))}
fn config_dir()->Result<PathBuf>{Ok(env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or(home()?.join(".config")).join("radminctl"))}
fn state_dir()->Result<PathBuf>{Ok(env::var_os("XDG_STATE_HOME").map(PathBuf::from).unwrap_or(home()?.join(".local/state")).join("radminctl"))}
fn pidfile()->Result<PathBuf>{Ok(state_dir()?.join("backend.pid"))}
fn backend()->Result<PathBuf>{
    let val=if let Some(v)=env::var_os("RADMINCTL_BACKEND"){PathBuf::from(v)}
        else {PathBuf::from(fs::read_to_string(config_dir()?.join("backend.path")).map_err(|_|"Set backend with: radminctl configure /opt/radmin-vpn-linux")?.trim())};
    let dir=fs::canonicalize(val)?;
    if !dir.join("run.sh").is_file() || !dir.join("lib.sh").is_file(){return Err("Backend lacks run.sh or lib.sh".into());}
    Ok(dir)
}
fn configure(args:&[String])->Result<()> {
    if args.len()!=1{return Err("Usage: radminctl configure DIRECTORY".into());}
    let path=fs::canonicalize(&args[0])?;
    if !path.join("run.sh").is_file()||!path.join("lib.sh").is_file(){return Err("Invalid backend directory".into());}
    let dir=config_dir()?;fs::create_dir_all(&dir)?;
    fs::write(dir.join("backend.path"),format!("{}\n",path.display()))?;
    println!("Backend configured: {}",path.display());Ok(())
}
fn cmdline_owned(pid:u32,root:&Path)->bool{
    let p=format!("/proc/{pid}/cmdline");
    let Ok(bytes)=fs::read(p) else{return false;};
    let a=root.join("run.sh").to_string_lossy().into_owned();
    let b=root.join("run_datacenter.sh").to_string_lossy().into_owned();
    bytes.split(|x|*x==0).any(|entry|entry==a.as_bytes()||entry==b.as_bytes())
}
fn tracked_pid(root:&Path)->Option<u32>{
    let pid=fs::read_to_string(pidfile().ok()?).ok()?.trim().parse().ok()?;
    if cmdline_owned(pid,root){Some(pid)}else{None}
}
fn tap_ip()->Option<String>{
    let out=Command::new("ip").args(["-4","-o","addr","show","dev",TAP]).output().ok()?;
    if !out.status.success(){return None;}
    let text=String::from_utf8_lossy(&out.stdout);
    let words:Vec<&str>=text.split_whitespace().collect();
    words.iter().position(|s|*s=="inet").and_then(|i|words.get(i+1).map(|s|s.to_string()))
}
fn status(args:&[String])->Result<()>{
    if !args.is_empty() && args!=["--json"]{return Err("Usage: radminctl status [--json]".into());}
    let root=backend()?;let pid=tracked_pid(&root);let ip=tap_ip();
    if args.first().is_some_and(|x|x=="--json"){
        println!("{{\"running\":{},\"pid\":{},\"interface\":\"{}\",\"ip\":{}}}",
            pid.is_some(),pid.map(|x|x.to_string()).unwrap_or("null".into()),TAP,
            ip.map(|x|json(&x)).unwrap_or("null".into()));
    }else{
        println!("Backend: {}\nRunning: {}\nInterface: {}\nVPN IP: {}",
            root.display(),pid.map(|p|p.to_string()).unwrap_or("no".into()),
            if Path::new("/sys/class/net/radminvpn0").exists(){TAP}else{"missing"},
            ip.unwrap_or("unassigned".into()));
    }Ok(())
}
fn start(sub:&str,args:&[String])->Result<()>{
    let root=backend()?;
    if let Some(pid)=tracked_pid(&root){return Err(format!("Already running: PID {pid}").into());}
    let web=sub=="web-ui";
    let script=if web{"run_datacenter.sh"}else{"run.sh"};
    if !root.join(script).is_file(){return Err(format!("Backend lacks {script}").into());}
    let mut cmd=Command::new("bash");
    cmd.arg(root.join(script));
    if sub=="up"{cmd.arg("--no-ui").arg("--no-broadcast-routes");}
    if web{
        let pass=env::var("RADMINCTL_VNC_PASSWORD").map_err(|_|"RADMINCTL_VNC_PASSWORD is required")?;
        if pass.len()!=8||!pass.is_ascii(){return Err("VNC password must be exactly 8 ASCII bytes".into());}
        cmd.arg("--web-bind").arg("127.0.0.1").arg("--vnc-password").arg(pass);
        eprintln!("WARNING: The backend's VNC server also listens on TCP 5900. Firewall that port; use SSH tunnel for 6080.");
        eprintln!("The backend exposes the VNC password in process arguments; use only in an isolated test container.");
    }
    let mut i=0;
    while i<args.len(){
        match args[i].as_str(){
            "--installer"=>{i+=1;let file=args.get(i).ok_or("Missing installer path")?;let file=fs::canonicalize(file)?;if !file.is_file(){return Err("Invalid installer".into());}cmd.arg("--installer").arg(file);},
            "--web-port" if web=>{i+=1;let p:u16=args.get(i).ok_or("Missing port")?.parse()?;if p==0{return Err("Invalid port".into());}cmd.arg("--web-port").arg(p.to_string());},
            _=>return Err(format!("Unknown flag: {}",args[i]).into())
        }i+=1;
    }
    cmd.current_dir(&root).stdin(Stdio::inherit()).stdout(Stdio::inherit()).stderr(Stdio::inherit());
    let mut child=cmd.spawn()?;let pid=child.id();
    let dir=state_dir()?;fs::create_dir_all(&dir)?;
    if let Err(e)=fs::write(pidfile()?,pid.to_string()){let _=child.kill();let _=child.wait();return Err(e.into());}
    let result=child.wait();
    if fs::read_to_string(pidfile()?).ok().as_deref()==Some(&pid.to_string()){let _=fs::remove_file(pidfile()?);}
    let exit=result?;
    if !exit.success(){
        let _=save_failure_report(&format!("Backend {sub} exited: {exit}"));
        return Err(format!("Backend exited: {exit}").into());
    }
    Ok(())
}
fn stop()->Result<()>{
    let root=backend()?;
    match tracked_pid(&root){
        Some(pid)=>{
            let status=Command::new("kill").arg("-TERM").arg(pid.to_string()).status()?;
            if !status.success(){return Err("Failed to terminate tracked backend".into());}
            println!("SIGTERM sent to backend PID {pid}");
        },
        None=>println!("No tracked backend process is running")
    }Ok(())
}
fn json(s:&str)->String{
    let mut out=String::from("\"");
    for c in s.chars(){match c{'"'=>out.push_str("\\\""),'\\'=>out.push_str("\\\\"),'\n'=>out.push_str("\\n"),'\r'=>out.push_str("\\r"),'\t'=>out.push_str("\\t"),c if c<' '=>out.push_str(&format!("\\u{:04x}",c as u32)),c=>out.push(c)}}
    out.push('"');out
}
#[derive(Default)]
struct Report{items:Vec<(String,String,String,String)>}
impl Report{
    fn push(&mut self,id:&str,level:&str,detail:impl Into<String>,hint:&str){self.items.push((id.into(),level.into(),detail.into(),hint.into()));}
    fn failed(&self)->bool{self.items.iter().any(|x|x.1=="fail")}
    fn text(&self)->String{
        self.items.iter().map(|(id,level,detail,hint)|format!("[{level}] {id}: {detail}{}\n",if hint.is_empty(){String::new()}else{format!("\n  Hint: {hint}")})).collect()
    }
    fn json(&self)->String{
        let rows=self.items.iter().map(|(id,level,detail,hint)|
            format!("{{\"id\":{},\"level\":{},\"detail\":{},\"hint\":{}}}",json(id),json(level),json(detail),json(hint))).collect::<Vec<_>>().join(",");
        format!("{{\"version\":{},\"checks\":[{rows}]}}",json(env!("CARGO_PKG_VERSION")))
    }
}
fn exists_command(name:&str)->bool{
    env::var_os("PATH").map(|paths|env::split_paths(&paths).any(|p|p.join(name).is_file())).unwrap_or(false)
}
fn inspect(tap_test:bool)->Report{
    let mut r=Report::default();
    for name in ["bash","ip","wine","wineserver","python3"]{
        r.push(&format!("dependency.{name}"),if exists_command(name){"pass"}else{"fail"},
            if exists_command(name){"installed"}else{"missing"},"Install the missing backend dependency");
    }
    if exists_command("wine"){
        match Command::new("wine").arg("--version").output(){
            Ok(o)=>{
                let ver=String::from_utf8_lossy(&o.stdout).trim().to_owned();
                let major=ver.trim_start_matches("wine-").split('.').next().and_then(|n|n.parse::<u32>().ok()).unwrap_or(0);
                r.push("wine.version",if major>=11{"pass"}else{"fail"},ver,"Backend needs Wine 11+");
            },Err(e)=>r.push("wine.version","fail",e.to_string(),"Check Wine installation")
        }
    }
    match backend(){
        Ok(root)=>{
            r.push("backend.root","pass",root.display().to_string(),"");
            for binary in ["tap_bridge","rvpnnetmp.sys","adapter_hook.dll","rvpn_launcher.exe","rvpn_dnsfix.so","rvpn_reuseport.so"]{
                r.push(&format!("backend.{binary}"),if root.join("build").join(binary).is_file(){"pass"}else{"fail"},
                    if root.join("build").join(binary).is_file(){"found"}else{"missing"},"Run make in backend source");
            }
            r.push("backend.process",if tracked_pid(&root).is_some(){"pass"}else{"warn"},
                tracked_pid(&root).map(|p|format!("PID {p}")).unwrap_or("not started".into()),"Start with radminctl up");
            let log=root.join("wineprefix/drive_c/ProgramData/Famatech/Radmin VPN/service.log");
            if log.exists(){
                if let Ok(text)=read_tail(&log,8192,true){
                    let patterns=[("WSAEACCES","TCP bind/port reuse failure"),("NdisInitializeReadWriteLock","NDIS driver crash"),("adapter ready","adapter ready"),("Registered as","registration reached")];
                    for (needle,meaning) in patterns{if text.contains(needle){r.push(&format!("log.{}",needle.to_lowercase()),"warn",meaning,"Check backend and Wine compatibility");}}
                }
            }
        },Err(e)=>r.push("backend.root","fail",e.to_string(),"Run radminctl configure /path/to/backend")
    }
    let tun=fs::metadata("/dev/net/tun").map(|x|x.file_type().is_char_device()).unwrap_or(false);
    r.push("lxc.tun",if tun{"pass"}else{"fail"},if tun{"character device exists"}else{"missing /dev/net/tun"},"Bind /dev/net/tun into LXC; allow c 10:200 rwm");
    let cap=fs::read_to_string("/proc/self/status").ok().and_then(|x|
        x.lines().find_map(|line|line.strip_prefix("CapEff:").and_then(|hex|u64::from_str_radix(hex.trim(),16).ok()))).unwrap_or(0);
    r.push("lxc.cap_net_admin",if cap&(1<<12)!=0{"pass"}else{"warn"},
        if cap&(1<<12)!=0{"available"}else{"not available to this process"},"Try as container root; check LXC capability policy");
    if tap_test{
        if !tun||!exists_command("ip"){r.push("lxc.tap_test","fail","TAP cannot be tested","Provide /dev/net/tun and iproute2");}
        else{
            let dev=format!("rct{}",std::process::id());
            if Command::new("ip").args(["link","show","dev",&dev]).status().is_ok_and(|s|s.success()){
                r.push("lxc.tap_test","warn","test interface already exists","Retry");}
            else{
                let result=Command::new("ip").args(["tuntap","add","dev",&dev,"mode","tap"]).output();
                match result{
                    Ok(o) if o.status.success()=>{
                        let removed=Command::new("ip").args(["link","delete","dev",&dev]).status().is_ok_and(|s|s.success());
                        r.push("lxc.tap_test",if removed{"pass"}else{"fail"},if removed{"created and deleted TAP"}else{"failed to delete TAP"},"Check CAP_NET_ADMIN");
                    },
                    Ok(o)=>r.push("lxc.tap_test","fail",String::from_utf8_lossy(&o.stderr).trim().to_owned(),"Check AppArmor, cgroup and device permissions"),
                    Err(e)=>r.push("lxc.tap_test","fail",e.to_string(),"Check iproute2")
                }
            }
        }
    }
    if let Some(ip)=tap_ip(){r.push("vpn.ip","pass",ip,"");}else{r.push("vpn.ip","warn","VPN IPv4 not assigned","Start VPN; then verify TAP");}
    r
}
fn diagnose(args:&[String])->Result<()>{
    let r=inspect(false);
    match args{
        []=>print!("{}",r.text()),
        [flag] if flag=="--json"=>println!("{}",r.json()),
        [cmd,dir] if cmd=="bundle"=>save_bundle(Path::new(dir),&r,false)?,
        [cmd,dir,flag] if cmd=="bundle" && flag=="--include-logs"=>save_bundle(Path::new(dir),&r,true)?,
        _=>return Err("Usage: diagnose [--json] | diagnose bundle DIR [--include-logs]".into())
    }Ok(())
}
fn save_bundle(dir:&Path,r:&Report,include_logs:bool)->Result<()>{
    fs::create_dir(dir)?;
    fs::set_permissions(dir,fs::Permissions::from_mode(0o700))?;
    let file=dir.join("report.json");fs::write(&file,r.json())?;
    fs::set_permissions(&file,fs::Permissions::from_mode(0o600))?;
    if include_logs{
        if let Ok(root)=backend(){
            let log=root.join("wineprefix/drive_c/ProgramData/Famatech/Radmin VPN/service.log");
            if log.is_file(){
                let copy=dir.join("service.log");fs::copy(log,&copy)?;
                fs::set_permissions(copy,fs::Permissions::from_mode(0o600))?;
            }
        }
    }
    println!("Diagnostic report: {}",dir.display());Ok(())
}
fn save_failure_report(message:&str)->Result<()>{
    let dir=state_dir()?;fs::create_dir_all(&dir)?;
    let now=SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let path=dir.join(format!("failure-{now}"));
    let mut r=inspect(false);r.push("backend.exit","fail",message,"Review logs and environment");
    save_bundle(&path,&r,false)
}
fn lxc(args:&[String])->Result<()>{
    if args.iter().any(|x|x!="--tap-test"&&x!="--json"){return Err("Usage: lxc-check [--tap-test] [--json]".into());}
    let r=inspect(args.iter().any(|x|x=="--tap-test"));
    let items=Report{items:r.items.into_iter().filter(|x|x.0.starts_with("lxc.")).collect()};
    if args.iter().any(|x|x=="--json"){println!("{}",items.json());}else{print!("{}",items.text());}
    if items.failed(){return Err("LXC check failed".into());}
    Ok(())
}
fn read_tail(path:&Path,max_bytes:usize,utf16:bool)->Result<String>{
    let mut file=fs::File::open(path)?;let size=file.metadata()?.len();
    use io::{Seek,SeekFrom};
    let start=size.saturating_sub(max_bytes as u64)&!1;file.seek(SeekFrom::Start(start))?;
    let mut data=Vec::new();file.read_to_end(&mut data)?;
    let text=if utf16{
        let units=data.chunks_exact(2).map(|c|u16::from_le_bytes([c[0],c[1]])).collect::<Vec<_>>();
        String::from_utf16_lossy(&units)
    }else{String::from_utf8_lossy(&data).to_string()};
    Ok(if start==0{text}else{text.lines().skip(1).collect::<Vec<_>>().join("\n")})
}
fn logs(args:&[String])->Result<()>{
    let mut source="service";let mut n=50usize;let mut i=0;
    while i<args.len(){
        match args[i].as_str(){
            "--source"=>{i+=1;source=args.get(i).ok_or("Missing log source")?;},
            "--lines"=>{i+=1;n=args.get(i).ok_or("Missing line count")?.parse()?;if n==0||n>2000{return Err("--lines must be 1..2000".into());}},
            x=>return Err(format!("Unknown flag: {x}").into())
        }i+=1;
    }
    let root=backend()?;
    let (p,utf16)=match source{
        "service"=>(root.join("wineprefix/drive_c/ProgramData/Famatech/Radmin VPN/service.log"),true),
        "bridge"=>(PathBuf::from("/tmp/radmin_bridge.log"),false),
        "driver"=>(PathBuf::from("/tmp/radmin_driver.log"),false),
        "launcher"=>(PathBuf::from("/tmp/radmin_service.log"),false),
        "gui"=>(PathBuf::from("/tmp/radmin_gui.log"),false),
        _=>return Err("Unknown log source".into())
    };
    let text=read_tail(&p,4*1024*1024,utf16)?;
    let rows=text.lines().collect::<Vec<_>>();for line in rows.iter().skip(rows.len().saturating_sub(n)){println!("{line}");}
    Ok(())
}
fn ping(args:&[String])->Result<()>{
    if args.len()!=1{return Err("Usage: ping 26.x.x.x".into());}
    let addr:Ipv4Addr=args[0].parse()?;
    if addr.octets()[0]!=26{return Err("Expected Radmin VPN 26.0.0.0/8 address".into());}
    let s=Command::new("ping").args(["-c","4","-W","2",&addr.to_string()]).status()?;
    if !s.success(){return Err("Peer did not answer ping".into());}Ok(())
}
fn selftest()->Result<()>{
    let mut r=Report::default();r.push("test","pass","ok","");
    assert!(r.json().contains("\"level\":\"pass\""));
    assert_eq!(json("a\n\"b"),"\"a\\n\\\"b\"");
    println!("radminctl selftest passed");Ok(())
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]fn escaping(){assert_eq!(json("a\n\"b"),"\"a\\n\\\"b\"");}
    #[test]fn report_json(){let mut r=Report::default();r.push("x","fail","test","fix");assert!(r.failed());assert!(r.json().contains("\"id\":\"x\""));}
    #[test]fn test_ipv4(){assert_eq!("26.1.2.3".parse::<Ipv4Addr>().unwrap().octets()[0],26);}
}

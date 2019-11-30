# bk-over-ssh
back-over-ssh is a simple utility for backup files over a ssh connection. It's pretty simple behind the scene, at the first invoke it copy all files you specified, when invoke again it check if file had changed by it's length or last_modified or optional sha1 value, only download changed files.

```
[www.example.com] 4/8  154.45KB/s 53.34kB/224.24MB ------------------------------ 0% 23m
149.34KB/s 693.55kB/183.21MB ⠄    0% 19m   jdk-8u112-linux-x64.tar.gz
```
## supported platforms.
If rust can compile successfully on that platform it supports, tested under Windows10, Windows2008R2, Ubuntu18, Centos7, Centos6. For Windows platform has tested [win32 openssh](https://github.com/PowerShell/Win32-OpenSSH). 

## interconnect pattern
current support mode:  
![passive interconnect](https://raw.githubusercontent.com/jianglibo/bk-over-ssh/master/readme_imgs/passive_interconnect.jpg)<br/>
planned support mode:  
![passive interconnect](https://raw.githubusercontent.com/jianglibo/bk-over-ssh/master/readme_imgs/active_interconnect.jpg)<br/>

## pros and cons
pros:  
1. Single executable, zero dependency.
2. lightweight, easyly invoking as cron task.

cons:  
1. too simple, don't know how to handle constantly changing files.

## initialize commands.
```
.\bk-over-ssh.exe --console-log --vv --app-role pull_hub copy-executable .\data\pull-servers-conf\xx.xx.xx.xx.yml C:\Users\Administrator\bk-over-ssh
.\bk-over-ssh.exe --console-log --vv --app-role pull_hub sync-pull-dirs .\data\pull-servers-conf\xx.xx.xx.xx.yml
.\bk-over-ssh.exe --console-log --vv --app-role pull_hub create-remote-db .\data\pull-servers-conf\xx.xx.xx.xx.yml --force
```

## An example configuration
application configuration:  
```yml
role: pull_hub
archive_cmd: 
  - C:/Program Files/7-Zip/7z.exe
  - a
  - archive_file_name
  - files_and_dirs
data_dir: data
log_conf:
  log_file: output.log
  verbose_modules: []
    # - data_shape::server
mail_conf:
  from: xxx@gmail.com
  username: xxx@gmail.com
  password: password
  hostname: xxx.example.com
  port: 587
```

One server configuration file:  
```yml
role: pull_hub
id_rsa: /home/jianglibo/.ssh/id_rsa
id_rsa_pub: /home/jianglibo/.ssh/id_rsa.pub
host: 127.0.0.1
port: 22
username: jianglibo
password: ~
auth_method: IdentityFile # Password, Agent, IdentityFile.
remote_exec: /home/osboxes/ws/bk-over-ssh/target/debug/bk-over-ssh
remote_server_yml: /home/osboxes/ws/bk-over-ssh/data/servers/localhost.yml
file_list_file: /home/jianglibo/file_list_file.txt
buf_len: 8192
use_db: true
skip_sha1: true
sql_batch_size: 50000
exclude_by_sql: [] # selected item will delete from database, that's as if excluded too.
#SELECT id FROM relative_file_item WHERE path LIKE '%.zip' ORDER BY path DESC LIMIT 100000 OFFSET 1 # both limit and offset are required.
rsync:
  window: 4096
  valve: 419430400 #if file length greater than this value, will use rsync agrithm to transfer file.
  sig_ext: .sig
  delta_ext: .delta
directories:
  - remote_dir: /home/jianglibo/ws/bk-over-ssh/fixtures/adir
    local_dir: ~
    includes:
      - "*.txt"
      - "*.png"
    excludes:
      - "*.log"
      - "*.bak"
archive_prefix: backup
archive_postfix: .7z
compress_archive: bzip2
prune_strategy:
  yearly: 1
  monthly: 1
  weekly: 1
  daily: 3
  hourly: 1
  minutely: 1
schedules: # this is a very special schedule implementation. you can execute this command line application at fixed intervals, when the scheduled time meets it execute or else it just skiped.
  - name: "sync-pull-dirs"
    # at 0 seconds, 30 minutes, 9,12,15 hours, may to august, monday, Wednesday, Friday, 2018 start every 2 years.
    cron: "0 30 9,12,15 1,15 May-Aug Mon,Wed,Fri 2018/2"
  - name: "archive-local"
    # at 0 seconds, 30 minutes, 9,12,15 hours, may to august, monday, Wednesday, Friday, 2018 start every 2 years.
    cron: "0 30 9,12,15 1,15 May-Aug Mon,Wed,Fri 2018/2"
```
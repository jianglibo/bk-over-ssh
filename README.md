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
planed support mode:  
![passive interconnect](https://raw.githubusercontent.com/jianglibo/bk-over-ssh/master/readme_imgs/active_interconnect.jpg)<br/>

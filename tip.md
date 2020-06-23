## deploy new release
1. git tag v20200623
2. git push origin --tags
3. download from github.
## windows openssh
executable location: C:\Windows\System32\OpenSSH-Win64
environment variables: Get-Childitem env:
https://blog.ropnop.com/extracting-ssh-private-keys-from-windows-10-ssh-agent/
Get-Service ssh-agent

## First stage
* list files
* compare files
* download files

## workflow

git checkout -b develop, git checkout develop

# workflow in different roles.
The role property in the app_conf file will affect the workflow of application.

## PullHub
1. connect to remote server
2. list remote files.
3. download remote files.
4. download each changed file from remote server.
5. confirm success.

## PassiveLeaf
1. list local files. write to a file specified in the configuration file.
2. do the confirm action when PullHub request.

## ReceiveHub
Even don't need this program!
Every user will create a linux account and a home directory. For example: useradd -m -d /var/www/tarunika -s /bin/bash -c "TecMint Technical Writer" -g 1000 tarunika.
1. create a new user.
2. upload new ssh key.

## ActiveLeaf
1. list local file.
2. upload changed file.

## common commands example
.\target\debug\bk-over-ssh.exe --console-log --app-role pull_hub create-db .\data\pull-servers-conf\localhost.yml

.\target\debug\bk-over-ssh.exe --console-log --app-role pull_hub sync-pull-dirs .\data\pull-servers-conf\localhost.yml

.\target\debug\bk-over-ssh.exe --console-log --as-service --app-role pull_hub sync-pull-dirs .\data\pull-servers-conf\localhost.yml


.\target\debug\bk-over-ssh.exe --console-log --as-service --app-role active_leaf sync-push-dirs .\data\push-conf\localhost.yml

.\target\debug\bk-over-ssh.exe --console-log --app-role active_leaf sync-push-dirs .\data\push-conf\localhost.yml

.\target\debug\bk-over-ssh.exe --console-log --vv  --app-role pull_hub create-remote-db .\data\pull-servers-conf\go2wheel.yml --force

.\target\debug\bk-over-ssh.exe --console-log --vv  --app-role active_leaf sync-push-dirs .\data\push-conf\go2wheel.yml --force

## window service.
https://github.com/kohsuke/winsw/

.\WinSW.NET4.exe status



## ActiveLeaf mode sync details.
1. fetch the file list from the remote pair. compare to the file list in local db. These two should have same count and each item should have same len. considering compare count only.
2. list local changed files plus the differences from the step 1.

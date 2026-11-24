$p = Start-Process -FilePath wsl.exe -ArgumentList '-d','AInodeProbe','-u','root','--cd','/','--','bash','/mnt/c/Repos/ainode/ainode/lab/windows/in-wsl/modeld-fg.sh' -PassThru -WindowStyle Hidden
Start-Sleep 5
"before: " + (wsl.exe -d AInodeProbe -u root --cd / -- bash /mnt/c/Repos/ainode/ainode/lab/windows/in-wsl/modeld-ps.sh)
Stop-Process -Id $p.Id -Force
Start-Sleep 4
"after killing wsl.exe: " + (wsl.exe -d AInodeProbe -u root --cd / -- bash /mnt/c/Repos/ainode/ainode/lab/windows/in-wsl/modeld-ps.sh)

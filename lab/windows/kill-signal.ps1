$p = Start-Process -FilePath wsl.exe -ArgumentList '-d','AInodeProbe','-u','root','--cd','/','--','bash','/mnt/c/Repos/ainode/ainode/lab/windows/in-wsl/trap.sh' -PassThru -WindowStyle Hidden
Start-Sleep 3
Stop-Process -Id $p.Id -Force
Start-Sleep 3
"signal seen by the Linux side: " + (wsl.exe -d AInodeProbe -u root --cd / -- sh -c "cat /tmp/sig 2>/dev/null || echo none-SIGKILL")

; AInode's NSIS hooks (tauri.windows.conf.json). The engine runs inside a WSL
; distribution named AInode that the app imported on its first Start
; (src/wsl.rs). It holds no user data (models, chats and settings stay in
; %LOCALAPPDATA%\AInode\data), so removing the app removes it too.
;
; The installer is a 32-bit program: $SYSDIR is SysWOW64 there, which has no
; wsl.exe (observed 2026-10-13: the distribution survived the uninstall).
; Sysnative is the real System32 as a 32-bit process sees it.
;
; Start with Windows (src/login.rs) is the value AInode under the user's Run
; key; it goes with the app. An update (/UPDATE) never runs the uninstaller,
; so the value stays across updates.
; An update writes the new files over the old ones and removes nothing
; (Tauri's installer template has no clean-up step before its File
; instructions, read 2026-11-23; and, as said above, an update does not run
; the uninstaller). AInode 0.8.4 and older put an
; OpenSSL tool and two libraries into the engine folder that this version no
; longer ships. The engine is started with LD_LIBRARY_PATH on engine\lib, so
; an old libstdc++.so.6 left there would be loaded in place of the WSL
; image's own: a file that is not in MANIFEST.json. They go before the new
; files are written. Delete does nothing when a file is not there. The app
; removes the same five names at engine start as well (engine.rs
; remove_old_engine_files), for an install this hook did not run on.
; Written on a Mac, not run: Track W checks it (a real 0.8.4, then the
; update, then the engine folder against MANIFEST.json).
!macro NSIS_HOOK_PREINSTALL
  Delete "$INSTDIR\resources\engine\openssl"
  Delete "$INSTDIR\resources\engine\openssl.sh"
  Delete "$INSTDIR\resources\engine\openssl.cnf"
  Delete "$INSTDIR\resources\engine\lib\libstdc++.so.6"
  Delete "$INSTDIR\resources\engine\lib\libgcc_s.so.1"
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  nsExec::Exec '"$WINDIR\Sysnative\wsl.exe" --unregister AInode'
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "AInode"
!macroend

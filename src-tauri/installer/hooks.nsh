; Atlas stops its bundled Ollama when it exits, but a crash or a forced close can
; leave it running and holding AtlasAI\ollama.exe and the model open. Stop only
; the copy that lives in this installation so the files can be replaced; an
; Ollama the user installed separately is left alone.
!macro AtlasStopBundledAi
  nsExec::Exec `powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "Get-Process -Name ollama -ErrorAction SilentlyContinue | Where-Object { $$_.Path -and $$_.Path.StartsWith('$INSTDIR\AtlasAI\', [StringComparison]::OrdinalIgnoreCase) } | Stop-Process -Force"`
  Pop $0
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro AtlasStopBundledAi
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro AtlasStopBundledAi
!macroend

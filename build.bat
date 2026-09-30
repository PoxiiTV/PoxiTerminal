@echo off
rem Genera PoxiTerminal para Windows x64: ZIP portable + instalador en dist\
rem Requisitos: Rust (rustup), Visual Studio Build Tools e Inno Setup 6.
setlocal
cd /d "%~dp0"

for /f "usebackq delims=" %%v in (`powershell -NoProfile -Command "(Select-String -Path nebula_app\Cargo.toml -Pattern '^version = \"(.+)\"' | Select-Object -First 1).Matches[0].Groups[1].Value"`) do set VERSION=%%v
echo === PoxiTerminal v%VERSION% ===

echo [1/3] Preparando runtime de ConPTY...
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\prepare-windows-runtime.ps1 -Destination assets\windows\conhost || goto :error

echo [2/3] Compilando y creando ZIP portable...
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\package-release.ps1 -Version %VERSION% -Force || goto :error

echo [3/3] Creando instalador...
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\build-installer.ps1 -Version %VERSION% -SkipBuild -Force || goto :error

rem El exe ronda los 120 MB sin comprimir y el ZIP unos 35 MB. Si crece mucho, algo va mal.
powershell -NoProfile -Command "Get-ChildItem dist\PoxiTerminal-v%VERSION%-* | ForEach-Object { '{0,-50} {1,6:N1} MB' -f $_.Name, ($_.Length / 1MB); if ($_.Length -gt 60MB) { Write-Warning ('{0} pesa mas de 60 MB, revisa el build' -f $_.Name) } }"
echo Listo: los paquetes estan en dist\
exit /b 0

:error
echo.
echo ERROR: el build ha fallado. Revisa los mensajes de arriba.
exit /b 1

@echo off
rem Arranca PoxiTerminal en modo desarrollo (compila si hace falta).
cd /d "%~dp0"
cargo run -p nebula --bin poxiterminal --features gpui-shell -- --gpui

@echo off
chcp 936 >nul
setlocal
set "PATH=%SystemRoot%\System32;%SystemRoot%;%SystemRoot%\System32\Wbem"
title RazerMate 后台 - 启动

set "EXE=%~dp0razermate-headless.exe"
for %%I in ("%~dp0profiles.json") do set "CFG=%%~fI"
set "LOG=%LOCALAPPDATA%\RazerMate\headless.log"

echo.
echo   RazerMate 后台 - 启动
echo   ----------------------------------------
echo.

if not exist "%EXE%" (
  echo   [错误] 找不到程序：
  echo          %EXE%
  echo.
  pause
  exit /b 1
)
if not exist "%CFG%" (
  echo   [错误] 找不到配置文件：
  echo          %CFG%
  echo.
  pause
  exit /b 1
)

tasklist /FI "IMAGENAME eq razermate-headless.exe" 2>nul | find /I "razermate-headless.exe" >nul
if not errorlevel 1 (
  echo   [提示] RazerMate 后台已经在运行，无需重复启动。
  echo          如需重启，请先运行 stop.cmd。
  echo.
  pause
  exit /b 0
)

echo   程序：%EXE%
echo   配置：%CFG%
echo   日志：%LOG%
echo.
echo   启动后常驻后台，按前台程序自动切换配置。
echo   只写 p0 临时层，不碰板载 Flash。
echo.
choice /C YN /N /M "  确认启动？[Y=启动 / N=取消] "
if errorlevel 2 (
  echo.
  echo   已取消，未做任何改动。
  timeout /t 3 >nul 2>&1
  exit /b 0
)

echo.
start "" "%EXE%" --run --config "%CFG%"
timeout /t 2 >nul 2>&1

tasklist /FI "IMAGENAME eq razermate-headless.exe" 2>nul | find /I "razermate-headless.exe" >nul
if errorlevel 1 (
  echo   [失败] 进程没有起来，请查看日志：
  echo          %LOG%
) else (
  echo   [成功] RazerMate 后台已启动，正在按前台程序切换配置。
)
echo.
timeout /t 3 >nul 2>&1
exit /b 0

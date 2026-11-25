@echo off
chcp 936 >nul
setlocal
set "PATH=%SystemRoot%\System32;%SystemRoot%;%SystemRoot%\System32\Wbem"
title RazerMate 后台 - 关闭

echo.
echo   RazerMate 后台 - 关闭
echo   ----------------------------------------
echo.

tasklist /FI "IMAGENAME eq razermate-headless.exe" 2>nul | find /I "razermate-headless.exe" >nul
if errorlevel 1 (
  echo   [提示] RazerMate 后台当前没有运行，无需关闭。
  echo.
  pause
  exit /b 0
)

echo   当前运行的实例：
echo.
tasklist /FI "IMAGENAME eq razermate-headless.exe" /FO TABLE /NH
echo.
echo   关闭后：鼠标不再随前台程序切换配置，p0 保持当前状态。
echo   注意：p0 是易失的，休眠或按物理 Profile 键后会回到板载配置，
echo         届时需要重新运行 start.cmd。
echo.
choice /C YN /N /M "  确认关闭？[Y=关闭 / N=取消] "
if errorlevel 2 (
  echo.
  echo   已取消，进程保持运行。
  timeout /t 3 >nul 2>&1
  exit /b 0
)

echo.
taskkill /IM razermate-headless.exe /F >nul 2>&1
timeout /t 1 >nul 2>&1

tasklist /FI "IMAGENAME eq razermate-headless.exe" 2>nul | find /I "razermate-headless.exe" >nul
if errorlevel 1 (
  echo   [成功] RazerMate 后台已关闭。
) else (
  echo   [失败] 进程仍在运行，可能需要管理员权限。
)
echo.
timeout /t 3 >nul 2>&1
exit /b 0

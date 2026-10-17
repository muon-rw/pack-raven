if exist "{{LAUNCHED}}" goto run
echo Installing the {{LOADER}} server. Its installer downloads the Minecraft server from Mojang.
rem The installer writes its own run scripts, so we install into a scratch folder and keep only its libraries.
if exist .install rmdir /s /q .install
mkdir .install
"%JAVA%" -jar "{{INSTALLER}}" --installServer .install || goto install_failed
if exist libraries rmdir /s /q libraries
move .install\libraries libraries >nul || goto install_failed
rmdir /s /q .install
goto run

:install_failed
if exist .install rmdir /s /q .install
echo Couldn't install the {{LOADER}} server. Check your internet connection, then run this script again.
pause
exit /b 1

if [ ! -f "{{LAUNCHED}}" ]; then
    echo "Installing the {{LOADER}} server. Its installer downloads the Minecraft server from Mojang."
    # The installer writes its own run scripts, so we install into a scratch folder and keep only its libraries.
    rm -rf .install && mkdir .install
    if "$JAVA" -jar "{{INSTALLER}}" --installServer .install && rm -rf libraries && mv .install/libraries libraries; then
        rm -rf .install
    else
        rm -rf .install
        echo "Couldn't install the {{LOADER}} server. Check your internet connection, then run this script again."
        exit 1
    fi
fi

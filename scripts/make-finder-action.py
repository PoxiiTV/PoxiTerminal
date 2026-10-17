#!/usr/bin/env python3
"""Genera la acción rápida de Finder «Abrir en PoxiTerminal».

Es un .workflow de Automator (servicio para carpetas) que ejecuta
`open -b dev.alexiis.poxiterminal <carpeta>`. Se instala con doble clic.

    python3 scripts/make-finder-action.py [carpeta-de-salida]
"""
import plistlib
import sys
import uuid
from pathlib import Path

NAME = "Abrir en PoxiTerminal"
SCRIPT = 'for carpeta in "$@"; do\n  open -b dev.alexiis.poxiterminal "$carpeta"\ndone\n'

out = Path(sys.argv[1] if len(sys.argv) > 1 else "packaging/macos") / f"{NAME}.workflow"
contents = out / "Contents"
contents.mkdir(parents=True, exist_ok=True)

info = {
    "NSServices": [{
        "NSMenuItem": {"default": NAME},
        "NSMessage": "runWorkflowAsService",
        "NSRequiredContext": {"NSApplicationIdentifier": "com.apple.finder"},
        "NSSendFileTypes": ["public.folder"],
    }],
}
with open(contents / "Info.plist", "wb") as f:
    plistlib.dump(info, f)

uid = lambda: str(uuid.uuid4()).upper()
action = {
    "action": {
        "AMAccepts": {"Container": "List", "Optional": True, "Types": ["com.apple.cocoa.string"]},
        "AMActionVersion": "2.0.3",
        "AMApplication": ["Automator"],
        "AMParameterProperties": {
            "COMMAND_STRING": {}, "CheckedForUserDefaultShell": {},
            "inputMethod": {}, "shell": {}, "source": {},
        },
        "AMProvides": {"Container": "List", "Types": ["com.apple.cocoa.string"]},
        "ActionBundlePath": "/System/Library/Automator/Run Shell Script.action",
        "ActionName": "Ejecutar script de shell",
        "ActionParameters": {
            "COMMAND_STRING": SCRIPT,
            "CheckedForUserDefaultShell": True,
            "inputMethod": 1,  # como argumentos ("$@")
            "shell": "/bin/zsh",
            "source": "",
        },
        "BundleIdentifier": "com.apple.RunShellScript",
        "CFBundleVersion": "2.0.3",
        "CanShowSelectedItemsWhenRun": False,
        "CanShowWhenRun": True,
        "Category": ["AMCategoryUtilities"],
        "Class Name": "RunShellScriptAction",
        "InputUUID": uid(),
        "Keywords": ["Shell", "Script", "Command", "Run", "Unix"],
        "OutputUUID": uid(),
        "UUID": uid(),
        "UnlocalizedApplications": ["Automator"],
        "arguments": {},
        "isViewVisible": 1,
        "location": "309.000000:253.000000",
        "nibPath": "/System/Library/Automator/Run Shell Script.action/Contents/Resources/Base.lproj/main.nib",
    },
    "isViewVisible": 1,
}
document = {
    "AMApplicationBuild": "523",
    "AMApplicationVersion": "2.10",
    "AMDocumentVersion": "2",
    "actions": [action],
    "connectors": {},
    "workflowMetaData": {
        "applicationBundleIDsByPath": {},
        "applicationPaths": [],
        "inputTypeIdentifier": "com.apple.Automator.fileSystemObject.folder",
        "outputTypeIdentifier": "com.apple.Automator.nothing",
        "presentationMode": 15,
        "processesInput": 0,
        "serviceApplicationBundleID": "com.apple.finder",
        "serviceApplicationPath": "/System/Library/CoreServices/Finder.app",
        "serviceInputTypeIdentifier": "com.apple.Automator.fileSystemObject.folder",
        "serviceOutputTypeIdentifier": "com.apple.Automator.nothing",
        "serviceProcessesInput": 0,
        "systemImageName": "NSActionTemplate",
        "useAutomaticInputType": 0,
        "workflowTypeIdentifier": "com.apple.Automator.servicesMenu",
    },
}
with open(contents / "document.wflow", "wb") as f:
    plistlib.dump(document, f)
print(f"Acción rápida guardada en {out}")

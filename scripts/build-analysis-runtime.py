"""Build the platform-native analysis worker for Tauri bundles."""
from pathlib import Path
import os
import subprocess
import sys
import tempfile

root=Path(__file__).resolve().parents[1]
output=root/'src-tauri'/'resources'/'runtime'
with tempfile.TemporaryDirectory(prefix='ml-runtime-') as temp:
    subprocess.run([sys.executable,'-m','PyInstaller','--noconfirm','--clean','--onedir',
        '--name','analysis-worker','--paths',str(root/'src-tauri'/'resources'),
        '--collect-all','sklearn','--distpath',str(output),'--workpath',str(Path(temp)/'build'),
        '--specpath',temp,str(root/'scripts'/'analysis-entry.py')],check=True)
exe=output/'analysis-worker'/('analysis-worker.exe' if os.name=='nt' else 'analysis-worker')
subprocess.run([str(exe),'--check'],check=True)
subprocess.run([sys.executable,str(root/"scripts"/"check-analysis-runtime.py"),str(exe)],check=True)

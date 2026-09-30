//! Git para el visor de cambios: «fotos» del árbol de trabajo, diff entre
//! fotos y descartar cambios de un archivo.
//!
//! La foto se hace con un **índice temporal** (copia del real + `add -A` +
//! `write-tree`): no toca el índice ni el stage del usuario, respeta el
//! `.gitignore` e incluye los archivos nuevos. Así «lo que cambió en este
//! turno» es simplemente `diff foto_inicio foto_ahora`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::display::side_panel::GitLocation;

/// Árbol vacío de Git: base para repos sin commits.
pub(crate) const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

/// Un parche mayor que esto se corta: el visor es para revisar, no para
/// cargar volcados enormes.
const MAX_PATCH_BYTES: usize = 16 * 1024 * 1024;
/// Líneas por archivo antes de truncar su diff.
const MAX_FILE_LINES: usize = 20_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FileStatus {
    Added,
    Deleted,
    Modified,
    Renamed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LineKind {
    Hunk,
    Context,
    Added,
    Removed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DiffLine {
    pub kind: LineKind,
    pub old_no: Option<u32>,
    pub new_no: Option<u32>,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FileDiff {
    pub path: String,
    /// Ruta anterior si es un renombrado.
    pub old_path: Option<String>,
    pub status: FileStatus,
    pub added: usize,
    pub removed: usize,
    pub binary: bool,
    pub truncated: bool,
    pub lines: Vec<DiffLine>,
}

fn git(location: &GitLocation, env: &[(&str, &str)], args: &[&str]) -> Result<Output, String> {
    let mut command = match location {
        GitLocation::Local { root } => {
            let mut command = Command::new("git");
            command
                .arg("-c")
                .arg(format!("safe.directory={}", root.display()))
                .arg("-c")
                .arg("core.quotepath=false")
                .arg("--literal-pathspecs")
                .arg("--no-optional-locks")
                .arg("-C")
                .arg(root);
            for (key, value) in env {
                command.env(key, value);
            }
            command
        },
        GitLocation::Wsl { distro, root } => {
            let mut command = Command::new("wsl.exe");
            // `--exec`: sin shell intermedio; con `--` WSL expandiría `$…` y `;`
            // de las rutas (inyección) y perdería los argumentos posicionales.
            command.args(["-d", distro, "--exec", "env"]);
            for (key, value) in env {
                command.arg(format!("{key}={value}"));
            }
            command.args([
                "git",
                "-c",
                "core.quotepath=false",
                "--literal-pathspecs",
                "--no-optional-locks",
                "-C",
                root,
            ]);
            command
        },
    };
    crate::platform::process::hidden_command(&mut command)
        .args(args)
        .output()
        .map_err(|error| format!("No se pudo ejecutar git: {error}"))
}

fn stdout_line(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn failure(output: &Output, fallback: &str) -> String {
    String::from_utf8_lossy(&output.stderr)
        .lines()
        .find(|line| !line.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| fallback.to_owned())
}

/// Sube desde cualquier carpeta del repo a su raíz.
pub(crate) fn repo_root(location: &GitLocation) -> Result<GitLocation, String> {
    let output = git(location, &[], &["rev-parse", "--show-toplevel"])?;
    if !output.status.success() {
        return Err("Esta carpeta no está dentro de un repositorio Git".to_owned());
    }
    let top = stdout_line(&output);
    Ok(match location {
        GitLocation::Local { .. } => GitLocation::Local { root: PathBuf::from(top) },
        GitLocation::Wsl { distro, .. } => GitLocation::Wsl { distro: distro.clone(), root: top },
    })
}

/// Árbol del último commit, o el árbol vacío si aún no hay commits.
pub(crate) fn head_tree(location: &GitLocation) -> String {
    git(location, &[], &["rev-parse", "--verify", "--quiet", "HEAD^{tree}"])
        .ok()
        .filter(|output| output.status.success())
        .map(|output| stdout_line(&output))
        .filter(|tree| !tree.is_empty())
        .unwrap_or_else(|| EMPTY_TREE.to_owned())
}

/// Foto del árbol de trabajo tal y como está ahora (con archivos nuevos no
/// ignorados), sin tocar el índice del usuario. Devuelve el hash del árbol.
pub(crate) fn snapshot_tree(location: &GitLocation) -> Result<String, String> {
    match location {
        GitLocation::Local { root } => snapshot_local(location, root),
        GitLocation::Wsl { distro, root } => {
            // Un solo wsl.exe: cada arranque cuesta.
            let script = r#"set -e
cd "$1"
t=$(mktemp)
trap 'rm -f "$t"' EXIT
cp "$(git rev-parse --git-path index)" "$t" 2>/dev/null || rm -f "$t"
GIT_INDEX_FILE="$t" git add -A >/dev/null
GIT_INDEX_FILE="$t" git write-tree"#;
            let mut command = Command::new("wsl.exe");
            let output = crate::platform::process::hidden_command(&mut command)
                .args(["-d", distro, "--exec", "sh", "-c", script, "poxiterminal", root])
                .output()
                .map_err(|error| format!("No se pudo ejecutar git en WSL: {error}"))?;
            if !output.status.success() {
                return Err(failure(&output, "No se pudo tomar la foto del repositorio"));
            }
            Ok(stdout_line(&output))
        },
    }
}

fn snapshot_local(location: &GitLocation, root: &Path) -> Result<String, String> {
    let index_path = git(location, &[], &["rev-parse", "--git-path", "index"])?;
    let real_index = root.join(stdout_line(&index_path));
    let temp = std::env::temp_dir().join(format!(
        "poxiterminal-index-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or_default()
    ));
    // Copiar el índice real conserva su caché de stat: `add -A` solo vuelve a
    // leer lo que ha cambiado. Sin índice (repo recién creado) se parte de cero.
    let _ = std::fs::copy(&real_index, &temp);
    let temp_text = temp.to_string_lossy().into_owned();
    let env = [("GIT_INDEX_FILE", temp_text.as_str())];
    let result = (|| {
        let added = git(location, &env, &["add", "-A"])?;
        if !added.status.success() {
            return Err(failure(&added, "No se pudo tomar la foto del repositorio"));
        }
        let tree = git(location, &env, &["write-tree"])?;
        if !tree.status.success() {
            return Err(failure(&tree, "No se pudo tomar la foto del repositorio"));
        }
        Ok(stdout_line(&tree))
    })();
    let _ = std::fs::remove_file(&temp);
    result
}

/// Diff entre dos árboles, ya troceado por archivo.
pub(crate) fn diff_trees(
    location: &GitLocation,
    base: &str,
    current: &str,
) -> Result<Vec<FileDiff>, String> {
    let output = git(
        location,
        &[],
        &[
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "-M",
            "--unified=3",
            base,
            current,
        ],
    )?;
    if !output.status.success() {
        return Err(failure(&output, "git diff falló"));
    }
    let mut bytes = output.stdout;
    bytes.truncate(MAX_PATCH_BYTES);
    Ok(parse_patch(&String::from_utf8_lossy(&bytes)))
}

/// Deja el archivo como estaba en `base`. Si no existía en `base`, lo manda a
/// la Papelera (nunca lo borra del todo). `unstage` quita también lo preparado
/// en el índice (modo «Todo sin commit»).
pub(crate) fn discard(
    location: &GitLocation,
    base: &str,
    file: &FileDiff,
    unstage: bool,
) -> Result<(), String> {
    let restore_from_base = |path: &str| -> Result<(), String> {
        let source = format!("--source={base}");
        let mut args = vec!["restore", source.as_str()];
        if unstage {
            args.push("--staged");
        }
        args.extend(["--worktree", "--", path]);
        let output = git(location, &[], &args)?;
        if output.status.success() { Ok(()) } else { Err(failure(&output, "git restore falló")) }
    };
    match file.status {
        FileStatus::Modified | FileStatus::Deleted => restore_from_base(&file.path),
        FileStatus::Added => {
            if unstage {
                let _ = git(location, &[], &["rm", "--cached", "--quiet", "--", &file.path]);
            }
            trash(location, &file.path)
        },
        FileStatus::Renamed => {
            if let Some(old) = &file.old_path {
                restore_from_base(old)?;
            }
            if unstage {
                let _ = git(location, &[], &["rm", "--cached", "--quiet", "--", &file.path]);
            }
            trash(location, &file.path)
        },
    }
}

fn trash(location: &GitLocation, relative: &str) -> Result<(), String> {
    match location {
        GitLocation::Local { root } => crate::display::send_to_recycle_bin(&root.join(relative)),
        GitLocation::Wsl { distro, root } => {
            // Papelera estándar de Linux (freedesktop): se puede recuperar.
            let script = r#"set -e
d="${XDG_DATA_HOME:-$HOME/.local/share}/Trash/files"
mkdir -p "$d"
n=$(basename "$2")
dest="$d/$n"
[ -e "$dest" ] && dest="$d/$n.$(date +%s)"
mv -- "$1/$2" "$dest""#;
            let mut command = Command::new("wsl.exe");
            let output = crate::platform::process::hidden_command(&mut command)
                .args(["-d", distro, "--exec", "sh", "-c", script, "poxiterminal", root, relative])
                .output()
                .map_err(|error| format!("No se pudo mover a la papelera en WSL: {error}"))?;
            if output.status.success() {
                Ok(())
            } else {
                Err(failure(&output, "No se pudo mover a la papelera"))
            }
        },
    }
}

/// Trocea la salida de `git diff` (formato unificado) por archivo.
pub(crate) fn parse_patch(patch: &str) -> Vec<FileDiff> {
    let mut files: Vec<FileDiff> = Vec::new();
    let (mut old_no, mut new_no) = (0u32, 0u32);
    for line in patch.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            let (a, b) = split_diff_header(rest);
            files.push(FileDiff {
                path: b,
                old_path: Some(a),
                status: FileStatus::Modified,
                added: 0,
                removed: 0,
                binary: false,
                truncated: false,
                lines: Vec::new(),
            });
            continue;
        }
        let Some(file) = files.last_mut() else { continue };
        if file.lines.is_empty() && !line.starts_with("@@") {
            // Cabecera del archivo.
            if line.starts_with("new file mode") {
                file.status = FileStatus::Added;
            } else if line.starts_with("deleted file mode") {
                file.status = FileStatus::Deleted;
            } else if let Some(from) = line.strip_prefix("rename from ") {
                file.status = FileStatus::Renamed;
                file.old_path = Some(from.to_owned());
            } else if let Some(to) = line.strip_prefix("rename to ") {
                file.path = to.to_owned();
            } else if line.starts_with("Binary files ") {
                file.binary = true;
            }
            continue;
        }
        if file.lines.len() >= MAX_FILE_LINES {
            file.truncated = true;
            continue;
        }
        if line.starts_with("@@") {
            (old_no, new_no) = hunk_starts(line);
            file.lines.push(DiffLine {
                kind: LineKind::Hunk,
                old_no: None,
                new_no: None,
                text: line.to_owned(),
            });
        } else if let Some(text) = line.strip_prefix('+') {
            file.added += 1;
            file.lines.push(DiffLine {
                kind: LineKind::Added,
                old_no: None,
                new_no: Some(new_no),
                text: text.to_owned(),
            });
            new_no += 1;
        } else if let Some(text) = line.strip_prefix('-') {
            file.removed += 1;
            file.lines.push(DiffLine {
                kind: LineKind::Removed,
                old_no: Some(old_no),
                new_no: None,
                text: text.to_owned(),
            });
            old_no += 1;
        } else if let Some(text) = line.strip_prefix(' ') {
            file.lines.push(DiffLine {
                kind: LineKind::Context,
                old_no: Some(old_no),
                new_no: Some(new_no),
                text: text.to_owned(),
            });
            old_no += 1;
            new_no += 1;
        }
        // "\ No newline at end of file" y demás se ignoran.
    }
    for file in &mut files {
        if file.status != FileStatus::Renamed {
            file.old_path = None;
        }
    }
    files
}

/// `a/ruta b/ruta` → (ruta, ruta). Las rutas con espacios son válidas porque
/// las dos mitades son iguales salvo en renombrados, que se corrigen después
/// con `rename from`/`rename to`.
fn split_diff_header(rest: &str) -> (String, String) {
    let rest = rest.strip_prefix("a/").unwrap_or(rest);
    match rest.rfind(" b/") {
        Some(split) => (rest[..split].to_owned(), rest[split + 3..].to_owned()),
        None => (rest.to_owned(), rest.to_owned()),
    }
}

/// `@@ -12,5 +14,7 @@ …` → (12, 14).
fn hunk_starts(line: &str) -> (u32, u32) {
    let mut old = 0;
    let mut new = 0;
    for part in line.split_whitespace() {
        let number = |text: &str| text.split(',').next().and_then(|n| n.parse().ok()).unwrap_or(0);
        if let Some(value) = part.strip_prefix('-') {
            old = number(value);
        } else if let Some(value) = part.strip_prefix('+') {
            new = number(value);
            break;
        }
    }
    (old, new)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATCH: &str = "diff --git a/src/main.rs b/src/main.rs
index 1111111..2222222 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,3 @@
 fn main() {
-    viejo();
+    nuevo();
 }
diff --git a/nuevo archivo.txt b/nuevo archivo.txt
new file mode 100644
index 0000000..3333333
--- /dev/null
+++ b/nuevo archivo.txt
@@ -0,0 +1,2 @@
+hola
+adiós
diff --git a/viejo.txt b/renombrado.txt
similarity index 90%
rename from viejo.txt
rename to renombrado.txt
diff --git a/logo.png b/logo.png
new file mode 100644
index 0000000..4444444
Binary files /dev/null and b/logo.png differ
diff --git a/borrado.rs b/borrado.rs
deleted file mode 100644
--- a/borrado.rs
+++ /dev/null
@@ -1 +0,0 @@
-adiós
";

    #[test]
    fn parse_patch_splits_files_and_numbers_lines() {
        let files = parse_patch(PATCH);
        assert_eq!(files.len(), 5);

        let main = &files[0];
        assert_eq!((main.path.as_str(), main.status), ("src/main.rs", FileStatus::Modified));
        assert_eq!((main.added, main.removed), (1, 1));
        assert_eq!(main.lines[0].kind, LineKind::Hunk);
        assert_eq!(
            main.lines[2],
            DiffLine {
                kind: LineKind::Removed,
                old_no: Some(2),
                new_no: None,
                text: "    viejo();".to_owned()
            }
        );
        assert_eq!(main.lines[3].new_no, Some(2));
        assert_eq!(main.lines[4].old_no, Some(3));

        assert_eq!(files[1].path, "nuevo archivo.txt");
        assert_eq!((files[1].status, files[1].added), (FileStatus::Added, 2));

        assert_eq!(files[2].status, FileStatus::Renamed);
        assert_eq!(files[2].old_path.as_deref(), Some("viejo.txt"));
        assert_eq!(files[2].path, "renombrado.txt");

        assert!(files[3].binary);
        assert_eq!((files[4].status, files[4].removed), (FileStatus::Deleted, 1));
        assert_eq!(files[4].old_path, None);
    }

    fn run(root: &Path, args: &[&str]) {
        let mut command = Command::new("git");
        let output = crate::platform::process::hidden_command(&mut command)
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{args:?}: {}", String::from_utf8_lossy(&output.stderr));
    }

    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        run(dir.path(), &["init", "-q"]);
        run(dir.path(), &["config", "user.email", "t@t"]);
        run(dir.path(), &["config", "user.name", "t"]);
        run(dir.path(), &["config", "core.autocrlf", "false"]);
        std::fs::write(dir.path().join("a.txt"), "uno\ndos\n").unwrap();
        std::fs::write(dir.path().join("preparado.txt"), "base\n").unwrap();
        run(dir.path(), &["add", "-A"]);
        run(dir.path(), &["commit", "-qm", "base"]);
        dir
    }

    #[test]
    fn turn_snapshot_diff_shows_only_what_changed_and_leaves_index_alone() {
        let dir = repo();
        let location = GitLocation::Local { root: dir.path().to_path_buf() };
        // Antes del turno el usuario ya tenía algo preparado y algo sin preparar.
        std::fs::write(dir.path().join("preparado.txt"), "base\nstage del usuario\n").unwrap();
        run(dir.path(), &["add", "preparado.txt"]);
        std::fs::write(dir.path().join("a.txt"), "uno\ndos\ntres\n").unwrap();

        let before = snapshot_tree(&location).unwrap();
        // La IA toca un archivo existente y crea otro.
        std::fs::write(dir.path().join("a.txt"), "UNO\ndos\ntres\n").unwrap();
        std::fs::write(dir.path().join("nuevo.rs"), "fn x() {}\n").unwrap();
        let after = snapshot_tree(&location).unwrap();

        let files = diff_trees(&location, &before, &after).unwrap();
        let paths: Vec<_> = files.iter().map(|file| (file.path.as_str(), file.status)).collect();
        assert_eq!(paths, [("a.txt", FileStatus::Modified), ("nuevo.rs", FileStatus::Added)]);
        assert_eq!((files[0].added, files[0].removed), (1, 1), "solo la línea que cambió la IA");

        // La foto no toca el índice real: lo preparado por el usuario sigue igual.
        run(dir.path(), &["diff", "--cached", "--quiet", "--", "a.txt"]);
        let staged = git(&location, &[], &["diff", "--cached", "--name-only"]).unwrap();
        assert_eq!(stdout_line(&staged), "preparado.txt");
    }

    #[test]
    fn discard_restores_modified_files_to_the_snapshot() {
        let dir = repo();
        let location = GitLocation::Local { root: dir.path().to_path_buf() };
        let before = snapshot_tree(&location).unwrap();
        std::fs::write(dir.path().join("a.txt"), "cambiado\n").unwrap();
        let after = snapshot_tree(&location).unwrap();
        let files = diff_trees(&location, &before, &after).unwrap();

        discard(&location, &before, &files[0], false).unwrap();
        assert_eq!(std::fs::read_to_string(dir.path().join("a.txt")).unwrap(), "uno\ndos\n");
        let now = snapshot_tree(&location).unwrap();
        assert!(diff_trees(&location, &before, &now).unwrap().is_empty());
    }

    #[test]
    fn head_tree_falls_back_to_the_empty_tree() {
        let dir = tempfile::tempdir().unwrap();
        run(dir.path(), &["init", "-q"]);
        let location = GitLocation::Local { root: dir.path().to_path_buf() };
        assert_eq!(head_tree(&location), EMPTY_TREE);
        std::fs::write(dir.path().join("x.txt"), "x\n").unwrap();
        let current = snapshot_tree(&location).unwrap();
        let files = diff_trees(&location, EMPTY_TREE, &current).unwrap();
        assert_eq!(files[0].status, FileStatus::Added);
    }
}

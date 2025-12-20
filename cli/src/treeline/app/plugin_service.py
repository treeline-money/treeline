"""Plugin management service."""

import json
import re
import shutil
import subprocess
import tempfile
from pathlib import Path
from typing import Dict, Any

import httpx

from treeline.domain import Result


class PluginService:
    """Service for managing external plugins."""

    def __init__(self, plugins_dir: Path):
        """Initialize plugin service.

        Args:
            plugins_dir: Directory where plugins are installed (e.g., ~/.treeline/plugins)
        """
        self.plugins_dir = Path(plugins_dir)
        self.plugins_dir.mkdir(parents=True, exist_ok=True)

    def create_plugin(
        self, name: str, target_dir: Path | None = None
    ) -> Result[Dict[str, Any]]:
        """Create a new plugin from template.

        Args:
            name: Plugin name (will be used as plugin ID and directory name)
            target_dir: Directory to create plugin in (defaults to current directory)

        Returns:
            Result with plugin directory path
        """
        # Validate name
        if not name or not name.replace("-", "").replace("_", "").isalnum():
            return Result(
                success=False,
                error="Plugin name must contain only letters, numbers, hyphens, and underscores",
            )

        # Determine target directory
        if target_dir is None:
            target_dir = Path.cwd()

        plugin_dir = target_dir / name
        if plugin_dir.exists():
            return Result(
                success=False, error=f"Directory already exists: {plugin_dir}"
            )

        # Find plugin template (relative to this service file)
        # The template is at: treeline-money/plugin-template/
        service_file = Path(__file__)  # cli/src/treeline/app/plugin_service.py
        repo_root = (
            service_file.parent.parent.parent.parent.parent
        )  # Go up to repo root
        template_dir = repo_root / "plugin-template"

        if not template_dir.exists():
            return Result(
                success=False, error=f"Plugin template not found at {template_dir}"
            )

        # Copy template
        try:
            shutil.copytree(
                template_dir,
                plugin_dir,
                ignore=shutil.ignore_patterns(
                    "node_modules", "dist", ".git", "*.log", "__pycache__"
                ),
            )
        except Exception as e:
            return Result(success=False, error=f"Failed to copy template: {e}")

        # Update manifest.json with plugin name and permissions
        manifest_path = plugin_dir / "manifest.json"
        # Convert name to table-safe format (replace hyphens with underscores)
        table_safe_name = name.replace("-", "_")
        display_name = name.replace("-", " ").replace("_", " ").title()
        try:
            with open(manifest_path, "r") as f:
                manifest = json.load(f)

            manifest["id"] = name
            manifest["name"] = display_name
            # Update permissions to use the new plugin ID
            manifest["permissions"] = {
                "tables": {"write": [f"sys_plugin_{table_safe_name}"]}
            }

            with open(manifest_path, "w") as f:
                json.dump(manifest, f, indent=2)
        except Exception as e:
            return Result(success=False, error=f"Failed to update manifest: {e}")

        # Update src/index.ts with plugin ID and permissions
        index_ts_path = plugin_dir / "src" / "index.ts"
        if index_ts_path.exists():
            try:
                content = index_ts_path.read_text()
                # Update the manifest in TypeScript to match
                content = content.replace('id: "hello-world"', f'id: "{name}"')
                content = content.replace(
                    'name: "Hello World"', f'name: "{display_name}"'
                )
                content = content.replace(
                    'write: ["sys_plugin_hello_world"]',
                    f'write: ["sys_plugin_{table_safe_name}"]',
                )
                index_ts_path.write_text(content)
            except Exception:
                # Non-fatal, manifest.json is the source of truth
                pass

        return Result(
            success=True,
            data={
                "plugin_dir": str(plugin_dir),
                "plugin_id": name,
            },
        )

    def install_plugin(
        self, source: str, version: str | None = None, force_build: bool = False
    ) -> Result[Dict[str, Any]]:
        """Install a plugin from a local directory or GitHub URL.

        Args:
            source: Local directory path or GitHub URL
            version: Version to install (for GitHub, e.g., "v1.0.0"). None = latest release.
            force_build: Force rebuild even if dist/index.js exists (only for local installs)

        Returns:
            Result with installation details
        """
        # Determine source type
        is_github = source.startswith(("http://", "https://", "git@"))

        if is_github:
            return self._install_from_github_release(source, version)
        else:
            return self._install_from_directory(Path(source).expanduser(), force_build)

    def _install_from_directory(
        self, source_dir: Path, force_build: bool
    ) -> Result[Dict[str, Any]]:
        """Install plugin from local directory.

        Args:
            source_dir: Path to plugin directory
            force_build: Force rebuild

        Returns:
            Result with installation details
        """
        if not source_dir.exists():
            return Result(success=False, error=f"Directory not found: {source_dir}")

        # Read manifest
        manifest_path = source_dir / "manifest.json"
        if not manifest_path.exists():
            return Result(
                success=False, error=f"No manifest.json found in {source_dir}"
            )

        try:
            with open(manifest_path, "r") as f:
                manifest = json.load(f)
        except Exception as e:
            return Result(success=False, error=f"Failed to parse manifest.json: {e}")

        plugin_id = manifest.get("id")
        if not plugin_id:
            return Result(success=False, error="manifest.json missing 'id' field")

        # Check if plugin needs to be built
        dist_file = source_dir / "dist" / "index.js"
        needs_build = force_build or not dist_file.exists()

        if needs_build:
            # Check if this looks like a source plugin (has package.json)
            if (source_dir / "package.json").exists():
                build_result = self._build_plugin(source_dir)
                if not build_result.success:
                    return build_result
            else:
                return Result(
                    success=False,
                    error=f"Plugin not built and no package.json found. Expected dist/index.js at {dist_file}",
                )

        # Verify dist file exists after build
        if not dist_file.exists():
            return Result(
                success=False,
                error=f"Build succeeded but dist/index.js not found at {dist_file}",
            )

        # Install to plugins directory
        install_dir = self.plugins_dir / plugin_id
        install_dir.mkdir(parents=True, exist_ok=True)

        # Copy manifest and dist
        try:
            shutil.copy2(manifest_path, install_dir / "manifest.json")
            shutil.copy2(dist_file, install_dir / "index.js")
        except Exception as e:
            return Result(success=False, error=f"Failed to copy plugin files: {e}")

        return Result(
            success=True,
            data={
                "plugin_id": plugin_id,
                "plugin_name": manifest.get("name", plugin_id),
                "version": manifest.get("version", "unknown"),
                "install_dir": str(install_dir),
                "built": needs_build,
            },
        )

    def _parse_github_url(self, url: str) -> tuple[str, str] | None:
        """Parse GitHub URL to extract owner and repo.

        Args:
            url: GitHub URL (https://github.com/owner/repo or git@github.com:owner/repo)

        Returns:
            Tuple of (owner, repo) or None if invalid
        """
        # Handle HTTPS URLs: https://github.com/owner/repo
        https_match = re.match(
            r"https?://github\.com/([^/]+)/([^/]+?)(?:\.git)?/?$", url
        )
        if https_match:
            return https_match.group(1), https_match.group(2)

        # Handle SSH URLs: git@github.com:owner/repo.git
        ssh_match = re.match(r"git@github\.com:([^/]+)/([^/]+?)(?:\.git)?$", url)
        if ssh_match:
            return ssh_match.group(1), ssh_match.group(2)

        return None

    def _install_from_github_release(
        self, url: str, version: str | None
    ) -> Result[Dict[str, Any]]:
        """Install plugin from GitHub release.

        Downloads pre-built manifest.json and index.js from release assets.

        Args:
            url: GitHub repository URL
            version: Version tag to install (e.g., "v1.0.0"). None = latest release.

        Returns:
            Result with installation details
        """
        # Parse GitHub URL
        parsed = self._parse_github_url(url)
        if not parsed:
            return Result(
                success=False,
                error=f"Invalid GitHub URL: {url}. Expected https://github.com/owner/repo",
            )

        owner, repo = parsed

        # Get release info from GitHub API
        if version:
            api_url = (
                f"https://api.github.com/repos/{owner}/{repo}/releases/tags/{version}"
            )
        else:
            api_url = f"https://api.github.com/repos/{owner}/{repo}/releases/latest"

        try:
            with httpx.Client(timeout=30) as client:
                response = client.get(
                    api_url,
                    headers={
                        "Accept": "application/vnd.github.v3+json",
                        "User-Agent": "Treeline-CLI",
                    },
                )
                if response.status_code == 404:
                    if version:
                        return Result(
                            success=False,
                            error=f"Release {version} not found for {owner}/{repo}",
                        )
                    else:
                        return Result(
                            success=False,
                            error=f"No releases found for {owner}/{repo}. The plugin author needs to create a release.",
                        )
                response.raise_for_status()
                release_data = response.json()
        except httpx.HTTPStatusError as e:
            return Result(success=False, error=f"GitHub API error: {e}")
        except httpx.RequestError as e:
            return Result(success=False, error=f"Network error: {e}")

        # Find manifest.json and index.js in release assets
        assets = {
            asset["name"]: asset["browser_download_url"]
            for asset in release_data.get("assets", [])
        }

        if "manifest.json" not in assets:
            return Result(
                success=False,
                error=f"Release {release_data.get('tag_name', 'unknown')} is missing manifest.json asset. "
                f"Available assets: {list(assets.keys()) or 'none'}",
            )

        if "index.js" not in assets:
            return Result(
                success=False,
                error=f"Release {release_data.get('tag_name', 'unknown')} is missing index.js asset. "
                f"Available assets: {list(assets.keys()) or 'none'}",
            )

        # Download files to temp directory
        with tempfile.TemporaryDirectory() as temp_dir:
            temp_path = Path(temp_dir)

            try:
                with httpx.Client(timeout=60, follow_redirects=True) as client:
                    # Download manifest.json
                    manifest_path = temp_path / "manifest.json"
                    response = client.get(assets["manifest.json"])
                    response.raise_for_status()
                    manifest_path.write_bytes(response.content)

                    # Download index.js
                    index_path = temp_path / "index.js"
                    response = client.get(assets["index.js"])
                    response.raise_for_status()
                    index_path.write_bytes(response.content)
            except httpx.RequestError as e:
                return Result(
                    success=False, error=f"Failed to download release assets: {e}"
                )

            # Read manifest to get plugin ID
            try:
                with open(manifest_path, "r") as f:
                    manifest = json.load(f)
            except Exception as e:
                return Result(
                    success=False, error=f"Failed to parse manifest.json: {e}"
                )

            plugin_id = manifest.get("id")
            if not plugin_id:
                return Result(success=False, error="manifest.json missing 'id' field")

            # Install to plugins directory
            install_dir = self.plugins_dir / plugin_id
            install_dir.mkdir(parents=True, exist_ok=True)

            try:
                shutil.copy2(manifest_path, install_dir / "manifest.json")
                shutil.copy2(index_path, install_dir / "index.js")
            except Exception as e:
                return Result(success=False, error=f"Failed to copy plugin files: {e}")

            return Result(
                success=True,
                data={
                    "plugin_id": plugin_id,
                    "plugin_name": manifest.get("name", plugin_id),
                    "version": release_data.get(
                        "tag_name", manifest.get("version", "unknown")
                    ),
                    "install_dir": str(install_dir),
                    "source": f"github:{owner}/{repo}",
                },
            )

    def _build_plugin(self, plugin_dir: Path) -> Result[None]:
        """Build a plugin using npm.

        Args:
            plugin_dir: Plugin directory

        Returns:
            Result indicating success or failure
        """
        # Check if npm is available
        try:
            subprocess.run(
                ["npm", "--version"],
                check=True,
                capture_output=True,
            )
        except (subprocess.CalledProcessError, FileNotFoundError):
            return Result(
                success=False,
                error="npm command not found. Please install Node.js and npm.",
            )

        # Install dependencies
        try:
            subprocess.run(
                ["npm", "install"],
                cwd=plugin_dir,
                check=True,
                capture_output=True,
                text=True,
            )
        except subprocess.CalledProcessError as e:
            return Result(success=False, error=f"npm install failed: {e.stderr}")

        # Build plugin
        try:
            subprocess.run(
                ["npm", "run", "build"],
                cwd=plugin_dir,
                check=True,
                capture_output=True,
                text=True,
            )
        except subprocess.CalledProcessError as e:
            return Result(success=False, error=f"npm run build failed: {e.stderr}")

        return Result(success=True)

    def uninstall_plugin(self, plugin_id: str) -> Result[Dict[str, Any]]:
        """Uninstall a plugin.

        Args:
            plugin_id: ID of plugin to uninstall

        Returns:
            Result with uninstallation details
        """
        plugin_dir = self.plugins_dir / plugin_id

        if not plugin_dir.exists():
            return Result(success=False, error=f"Plugin not found: {plugin_id}")

        # Read manifest for plugin name
        manifest_path = plugin_dir / "manifest.json"
        plugin_name = plugin_id
        if manifest_path.exists():
            try:
                with open(manifest_path, "r") as f:
                    manifest = json.load(f)
                    plugin_name = manifest.get("name", plugin_id)
            except Exception:
                pass  # Use plugin_id as fallback

        # Remove plugin directory
        try:
            shutil.rmtree(plugin_dir)
        except Exception as e:
            return Result(success=False, error=f"Failed to remove plugin: {e}")

        return Result(
            success=True,
            data={
                "plugin_id": plugin_id,
                "plugin_name": plugin_name,
            },
        )

    def fetch_manifest(
        self, url: str, version: str | None = None
    ) -> Result[Dict[str, Any]]:
        """Fetch manifest.json from a GitHub release without installing.

        Args:
            url: GitHub repository URL
            version: Version tag to fetch (e.g., "v1.0.0"). None = latest release.

        Returns:
            Result with manifest data and release info
        """
        # Parse GitHub URL
        parsed = self._parse_github_url(url)
        if not parsed:
            return Result(
                success=False,
                error=f"Invalid GitHub URL: {url}. Expected https://github.com/owner/repo",
            )

        owner, repo = parsed

        # Get release info from GitHub API
        if version:
            api_url = (
                f"https://api.github.com/repos/{owner}/{repo}/releases/tags/{version}"
            )
        else:
            api_url = f"https://api.github.com/repos/{owner}/{repo}/releases/latest"

        try:
            with httpx.Client(timeout=30) as client:
                response = client.get(
                    api_url,
                    headers={
                        "Accept": "application/vnd.github.v3+json",
                        "User-Agent": "Treeline-CLI",
                    },
                )
                if response.status_code == 404:
                    if version:
                        return Result(
                            success=False,
                            error=f"Release {version} not found for {owner}/{repo}",
                        )
                    else:
                        return Result(
                            success=False,
                            error=f"No releases found for {owner}/{repo}. The plugin author needs to create a release.",
                        )
                response.raise_for_status()
                release_data = response.json()
        except httpx.HTTPStatusError as e:
            return Result(success=False, error=f"GitHub API error: {e}")
        except httpx.RequestError as e:
            return Result(success=False, error=f"Network error: {e}")

        # Find manifest.json in release assets
        assets = {
            asset["name"]: asset["browser_download_url"]
            for asset in release_data.get("assets", [])
        }

        if "manifest.json" not in assets:
            return Result(
                success=False,
                error=f"Release {release_data.get('tag_name', 'unknown')} is missing manifest.json asset. "
                f"Available assets: {list(assets.keys()) or 'none'}",
            )

        # Download manifest.json
        try:
            with httpx.Client(timeout=30, follow_redirects=True) as client:
                response = client.get(assets["manifest.json"])
                response.raise_for_status()
                manifest = response.json()
        except httpx.RequestError as e:
            return Result(
                success=False, error=f"Failed to download manifest.json: {e}"
            )
        except json.JSONDecodeError as e:
            return Result(
                success=False, error=f"Failed to parse manifest.json: {e}"
            )

        return Result(
            success=True,
            data={
                "manifest": manifest,
                "version": release_data.get("tag_name", manifest.get("version", "unknown")),
                "source": f"github:{owner}/{repo}",
            },
        )

    def list_plugins(self) -> Result[list[Dict[str, Any]]]:
        """List all installed plugins.

        Returns:
            Result with list of plugin info
        """
        plugins = []

        if not self.plugins_dir.exists():
            return Result(success=True, data=plugins)

        for plugin_dir in self.plugins_dir.iterdir():
            if not plugin_dir.is_dir():
                continue

            manifest_path = plugin_dir / "manifest.json"
            if not manifest_path.exists():
                continue

            try:
                with open(manifest_path, "r") as f:
                    manifest = json.load(f)

                plugins.append(
                    {
                        "id": manifest.get("id", plugin_dir.name),
                        "name": manifest.get("name", plugin_dir.name),
                        "version": manifest.get("version", "unknown"),
                        "description": manifest.get("description", ""),
                        "author": manifest.get("author", ""),
                    }
                )
            except Exception:
                # Skip invalid plugins
                continue

        return Result(success=True, data=plugins)

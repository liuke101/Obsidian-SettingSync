using System.Collections.Generic;
using System.IO;
using ObsidianSettingSync.Models;

namespace ObsidianSettingSync.Services;

public sealed class SyncService
{
    private readonly ILinkFileSystem _fileSystem;

    public SyncService(ILinkFileSystem fileSystem)
    {
        _fileSystem = fileSystem;
    }

    public IReadOnlyList<OperationResult> Execute(SyncOperation operation, string destinationPath, string sourcePath, bool isObsidianMode = false, HashSet<string>? exclusions = null, HashSet<string>? additionalDirs = null)
    {
        var results = new List<OperationResult>();
        var excludeSet = exclusions ?? new HashSet<string>(System.StringComparer.OrdinalIgnoreCase);
        var extraDirs = additionalDirs ?? new HashSet<string>(System.StringComparer.OrdinalIgnoreCase);

        if (operation == SyncOperation.Create)
        {
            if (isObsidianMode)
            {
                var srcObsidian = Path.Combine(sourcePath, ".obsidian");
                var destObsidian = Path.Combine(destinationPath, ".obsidian");
                
                if (_fileSystem.DirectoryExists(srcObsidian))
                {
                    if (!_fileSystem.DirectoryExists(destObsidian))
                    {
                        Directory.CreateDirectory(destObsidian);
                    }
                    
                    foreach (var dir in _fileSystem.GetDirectories(srcObsidian))
                    {
                        var name = Path.GetFileName(dir);
                        if (excludeSet.Contains(name)) continue;
                        results.Add(TryCreate($".obsidian/{name}", Path.Combine(destObsidian, name), dir, true));
                    }
                    
                    foreach (var file in _fileSystem.GetFiles(srcObsidian))
                    {
                        var name = Path.GetFileName(file);
                        if (excludeSet.Contains(name)) continue;
                        results.Add(TryCreate($".obsidian/{name}", Path.Combine(destObsidian, name), file, false));
                    }
                }
                
                foreach (var extraDir in extraDirs)
                {
                    if (excludeSet.Contains(extraDir)) continue;

                    var srcExtra = Path.Combine(sourcePath, extraDir);
                    if (_fileSystem.DirectoryExists(srcExtra))
                    {
                        results.Add(TryCreate(extraDir, Path.Combine(destinationPath, extraDir), srcExtra, true));
                    }
                }
            }
            else
            {
                foreach (var dir in _fileSystem.GetDirectories(sourcePath))
                {
                    var name = Path.GetFileName(dir);
                    if (excludeSet.Contains(name)) continue;
                    results.Add(TryCreate(name, Path.Combine(destinationPath, name), dir, true));
                }
                
                foreach (var file in _fileSystem.GetFiles(sourcePath))
                {
                    var name = Path.GetFileName(file);
                    if (excludeSet.Contains(name)) continue;
                    results.Add(TryCreate(name, Path.Combine(destinationPath, name), file, false));
                }
            }

            return results;
        }

        if (isObsidianMode)
        {
            var destObsidian = Path.Combine(destinationPath, ".obsidian");
            if (_fileSystem.DirectoryExists(destObsidian))
            {
                foreach (var dir in _fileSystem.GetDirectories(destObsidian))
                {
                    var name = Path.GetFileName(dir);
                    if (excludeSet.Contains(name)) continue;
                    results.Add(TryDelete($".obsidian/{name}", dir, true));
                }
                foreach (var file in _fileSystem.GetFiles(destObsidian))
                {
                    var name = Path.GetFileName(file);
                    if (excludeSet.Contains(name)) continue;
                    results.Add(TryDelete($".obsidian/{name}", file, false));
                }
            }

            foreach (var extraDir in extraDirs)
            {
                if (excludeSet.Contains(extraDir)) continue;

                var destExtra = Path.Combine(destinationPath, extraDir);
                results.Add(TryDelete(extraDir, destExtra, true));
            }
        }
        else
        {
            foreach (var dir in _fileSystem.GetDirectories(destinationPath))
            {
                var name = Path.GetFileName(dir);
                if (excludeSet.Contains(name)) continue;
                results.Add(TryDelete(name, dir, true));
            }

            foreach (var file in _fileSystem.GetFiles(destinationPath))
            {
                var name = Path.GetFileName(file);
                if (excludeSet.Contains(name)) continue;
                results.Add(TryDelete(name, file, false));
            }
        }

        return results;
    }

    private OperationResult TryCreate(string name, string destinationPath, string sourcePath, bool isDirectory)
    {
        try
        {
            if (isDirectory && !_fileSystem.DirectoryExists(sourcePath))
            {
                return new OperationResult(name, false, $"{name}: 源目录不存在");
            }

            if (!isDirectory && !_fileSystem.FileExists(sourcePath))
            {
                return new OperationResult(name, false, $"{name}: 源文件不存在");
            }

            if (isDirectory && _fileSystem.DirectoryExists(destinationPath))
            {
                if (!_fileSystem.IsReparsePoint(destinationPath))
                {
                    _fileSystem.DeleteDirectory(destinationPath, recursive: true);
                }
                else
                {
                    _fileSystem.DeleteDirectory(destinationPath);
                }
            }

            if (!isDirectory && _fileSystem.FileExists(destinationPath))
            {
                _fileSystem.DeleteFile(destinationPath);
            }

            if (isDirectory)
            {
                _fileSystem.CreateDirectorySymbolicLink(destinationPath, sourcePath);
            }
            else
            {
                _fileSystem.CreateFileSymbolicLink(destinationPath, sourcePath);
            }

            return new OperationResult(name, true, $"{name}: 符号链接创建成功");
        }
        catch (Exception ex)
        {
            return new OperationResult(name, false, $"{name}: 创建失败 - {ex.Message}");
        }
    }

    private OperationResult TryDelete(string name, string path, bool isDirectory)
    {
        try
        {
            if (isDirectory)
            {
                if (!_fileSystem.DirectoryExists(path))
                {
                    return new OperationResult(name, false, $"{name}: 目标目录不存在");
                }

                if (!_fileSystem.IsReparsePoint(path))
                {
                    return new OperationResult(name, false, $"{name}: 目标目录不是软链接，已跳过");
                }

                _fileSystem.DeleteDirectory(path);
                return new OperationResult(name, true, $"{name}: 符号链接删除成功");
            }

            if (!_fileSystem.FileExists(path))
            {
                return new OperationResult(name, false, $"{name}: 目标文件不存在");
            }

            if (!_fileSystem.IsReparsePoint(path))
            {
                return new OperationResult(name, false, $"{name}: 目标文件不是软链接，已跳过");
            }

            _fileSystem.DeleteFile(path);
            return new OperationResult(name, true, $"{name}: 符号链接删除成功");
        }
        catch (Exception ex)
        {
            return new OperationResult(name, false, $"{name}: 删除失败 - {ex.Message}");
        }
    }
}

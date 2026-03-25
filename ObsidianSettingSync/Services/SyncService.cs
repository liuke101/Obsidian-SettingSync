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

    public static IReadOnlyList<string> ConfigFileDirectories { get; } =
    [
        "plugins",
        "snippets",
        "themes"
    ];

    public static IReadOnlyList<string> JsonConfigFiles { get; } =
    [
        "app.json",
        "appearance.json",
        "backlink.json",
        "bookmarks.json",
        "canvas.json",
        "community-plugins.json",
        "core-plugins.json",
        "core-plugins-migration.json",
        "daily-notes.json",
        "file-recovery.json",
        "graph.json",
        "hotkeys.json",
        "note-composer.json",
        "page-preview.json",
        "templates.json",
        "types.json"
        //"workspace.json" 该文件不进行软链接
    ];

    public IReadOnlyList<OperationResult> Execute(SyncOperation operation, string destinationPath, string sourcePath)
    {
        var results = new List<OperationResult>();

        if (operation == SyncOperation.Create)
        {
            foreach (var directory in ConfigFileDirectories)
            {
                var dest = Path.Combine(destinationPath, directory);
                var src = Path.Combine(sourcePath, directory);
                results.Add(TryCreate(directory, dest, src, isDirectory: true));
            }

            foreach (var file in JsonConfigFiles)
            {
                var dest = Path.Combine(destinationPath, file);
                var src = Path.Combine(sourcePath, file);
                results.Add(TryCreate(file, dest, src, isDirectory: false));
            }

            return results;
        }

        foreach (var directory in ConfigFileDirectories)
        {
            var path = Path.Combine(destinationPath, directory);
            results.Add(TryDelete(directory, path, isDirectory: true));
        }

        foreach (var file in JsonConfigFiles)
        {
            var path = Path.Combine(destinationPath, file);
            results.Add(TryDelete(file, path, isDirectory: false));
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
                    return new OperationResult(name, false, $"{name}: 目标目录已存在且不是软链接");
                }

                _fileSystem.DeleteDirectory(destinationPath);
            }

            if (!isDirectory && _fileSystem.FileExists(destinationPath))
            {
                if (!_fileSystem.IsReparsePoint(destinationPath))
                {
                    return new OperationResult(name, false, $"{name}: 目标文件已存在且不是软链接");
                }

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

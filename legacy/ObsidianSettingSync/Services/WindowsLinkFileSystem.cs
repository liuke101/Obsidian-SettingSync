using System.IO;

namespace ObsidianSettingSync.Services;

public sealed class WindowsLinkFileSystem : ILinkFileSystem
{
    public bool DirectoryExists(string path) => Directory.Exists(path);

    public bool FileExists(string path) => File.Exists(path);

    public bool IsReparsePoint(string path)
    {
        if (!DirectoryExists(path) && !FileExists(path))
        {
            return false;
        }

        var attrs = File.GetAttributes(path);
        return (attrs & FileAttributes.ReparsePoint) == FileAttributes.ReparsePoint;
    }

    public void CreateDirectorySymbolicLink(string destinationPath, string sourcePath)
    {
        Directory.CreateSymbolicLink(destinationPath, sourcePath);
    }

    public void CreateFileSymbolicLink(string destinationPath, string sourcePath)
    {
        File.CreateSymbolicLink(destinationPath, sourcePath);
    }

    public void DeleteDirectory(string path, bool recursive = false)
    {
        Directory.Delete(path, recursive);
    }

    public void DeleteFile(string path)
    {
        File.Delete(path);
    }

    public string[] GetDirectories(string path)
    {
        return Directory.GetDirectories(path);
    }

    public string[] GetFiles(string path)
    {
        return Directory.GetFiles(path);
    }
}

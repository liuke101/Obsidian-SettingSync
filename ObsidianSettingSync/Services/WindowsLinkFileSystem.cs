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

    public void DeleteDirectory(string path)
    {
        Directory.Delete(path, recursive: false);
    }

    public void DeleteFile(string path)
    {
        File.Delete(path);
    }
}

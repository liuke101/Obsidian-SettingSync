namespace ObsidianSettingSync.Services;

public interface ILinkFileSystem
{
    bool DirectoryExists(string path);
    bool FileExists(string path);
    bool IsReparsePoint(string path);
    void CreateDirectorySymbolicLink(string destinationPath, string sourcePath);
    void CreateFileSymbolicLink(string destinationPath, string sourcePath);
    void DeleteDirectory(string path, bool recursive = false);
    void DeleteFile(string path);
    string[] GetDirectories(string path);
    string[] GetFiles(string path);
}

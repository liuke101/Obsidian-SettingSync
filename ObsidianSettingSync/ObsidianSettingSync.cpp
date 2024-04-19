#include<iostream>
#include<vector>
#include<string>
#include <windows.h>
#include <winbase.h>
using namespace std;

//目录
vector<string> configFileDirectories =
{
    "plugins",
    "snippets",
    "themes"
};

//json文件
vector<string> jsonConfigFiles =
{
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
};

int main()
{
    cout << "Select operation: [0] create soft link, [1] removes soft link." << endl;
    //输入0则是创建链接，输入1则是删除链接
    int operation;
    cin >> operation;

    if (operation == 0 || operation == 1)
    {
        string Dest_path;
        string Src_path;
        // 请输入目标仓库.Obsidian文件夹路径
        cout << "Destination repository [.Obsidian] folder path:";
        cin >> Dest_path;
        // 请输入源仓库.Obsidian文件夹路径
        cout << "Source repository [.Obsidian] folder path:";
        cin >> Src_path;

    
        if (operation == 0)
        {
            for (auto FileDir : configFileDirectories)
            {
                //https://learn.microsoft.com/zh-cn/windows/win32/fileio/creating-symbolic-links
                int flag = CreateSymbolicLinkA((Dest_path + "\\" + FileDir).c_str(), (Src_path + "\\" + FileDir).c_str(), SYMBOLIC_LINK_FLAG_DIRECTORY);
                if (flag == 0)
                {
                    string successLog = FileDir + "Failed to create symbolic link!";
                    cout << successLog << endl;
                }
                else
                {
                    string errorLog = FileDir + "Symbolic link created successfully!";
                    cout << errorLog << endl;
                }
            }

            for (auto jsonFile : jsonConfigFiles)
            {
                int flag = CreateSymbolicLinkA((Dest_path + "\\" + jsonFile).c_str(), (Src_path + "\\" + jsonFile).c_str(), 0);
                if (flag == 0)
                {
                    string successLog = jsonFile + "Failed to create symbolic link!";
                    cout << successLog << endl;
                }
                else
                {
                    string errorLog = jsonFile + "Symbolic link created successfully!";
                    cout << errorLog << endl;

                    
                }
            }
        }
        else if (operation == 1)
        {
            //合并两个vector
            configFileDirectories.insert(configFileDirectories.end(), jsonConfigFiles.begin(), jsonConfigFiles.end());
            //删除软链接
            for (auto File : configFileDirectories)
            {
                int flag = RemoveDirectoryA((Dest_path + "\\" + File).c_str());
                if (flag == 0)
                {
                    string successLog = File + "Failed to remove symbolic link!";
                    cout << successLog << endl;
                }
                else
                {
                    string errorLog = File + "Symbolic link removed successfully!";
                    cout << errorLog << endl;
                }
            }
        }
    }
    else
    {
        cout << "Invalid operation!" << endl;
    }

    system("pause");
    return 0;
}

using System.IO;
using System.Linq;
using System.Windows;
using ObsidianSettingSync.Models;
using ObsidianSettingSync.Services;
using Forms = System.Windows.Forms;

namespace ObsidianSettingSync;

public partial class MainWindow : Window
{
    private readonly SyncService _syncService;
    private SyncOperation _currentOperation = SyncOperation.Create;

    public MainWindow()
    {
        InitializeComponent();
        _syncService = new SyncService(new WindowsLinkFileSystem());
        UpdateOperationState(SyncOperation.Create);
        OperationPanel.Visibility = Visibility.Collapsed;
        HomePanel.Visibility = Visibility.Visible;
    }

    private void CreateModeButton_OnClick(object sender, RoutedEventArgs e)
    {
        if (!Confirm("将进入“创建软连接”步骤，是否继续？"))
        {
            return;
        }

        UpdateOperationState(SyncOperation.Create);
        EnterOperationMode();
    }

    private void DeleteModeButton_OnClick(object sender, RoutedEventArgs e)
    {
        if (!Confirm("将进入“删除软连接”步骤，是否继续？"))
        {
            return;
        }

        UpdateOperationState(SyncOperation.Delete);
        EnterOperationMode();
    }

    private void BrowseDestinationButton_OnClick(object sender, RoutedEventArgs e)
    {
        if (!Confirm("选择目标仓库 .Obsidian 文件夹，是否继续？"))
        {
            return;
        }

        var path = PickFolder();
        if (!string.IsNullOrWhiteSpace(path))
        {
            DestinationPathTextBox.Text = path;
        }
    }

    private void BrowseSourceButton_OnClick(object sender, RoutedEventArgs e)
    {
        if (!Confirm("选择源仓库 .Obsidian 文件夹，是否继续？"))
        {
            return;
        }

        var path = PickFolder();
        if (!string.IsNullOrWhiteSpace(path))
        {
            SourcePathTextBox.Text = path;
        }
    }

    private void ExecuteButton_OnClick(object sender, RoutedEventArgs e)
    {
        if (!Confirm("将开始执行当前操作，是否继续？"))
        {
            return;
        }

        var destinationPath = DestinationPathTextBox.Text.Trim();
        var sourcePath = SourcePathTextBox.Text.Trim();

        if (string.IsNullOrWhiteSpace(destinationPath) || string.IsNullOrWhiteSpace(sourcePath))
        {
            System.Windows.MessageBox.Show("目标路径和源路径都不能为空。", "输入校验", MessageBoxButton.OK, MessageBoxImage.Warning);
            return;
        }

        if (!Directory.Exists(destinationPath))
        {
            System.Windows.MessageBox.Show("目标路径不存在，请重新选择。", "输入校验", MessageBoxButton.OK, MessageBoxImage.Warning);
            return;
        }

        if (!Directory.Exists(sourcePath))
        {
            System.Windows.MessageBox.Show("源路径不存在，请重新选择。", "输入校验", MessageBoxButton.OK, MessageBoxImage.Warning);
            return;
        }

        if (!LooksLikeObsidianFolder(destinationPath) || !LooksLikeObsidianFolder(sourcePath))
        {
            if (!Confirm("所选目录不以 .Obsidian 结尾，仍然继续执行吗？"))
            {
                return;
            }
        }

        LogListBox.Items.Clear();

        var results = _syncService.Execute(_currentOperation, destinationPath, sourcePath);
        foreach (var result in results)
        {
            LogListBox.Items.Add(result.Message);
        }

        var successCount = results.Count(r => r.IsSuccess);
        var failedCount = results.Count - successCount;
        System.Windows.MessageBox.Show($"执行完成。成功 {successCount} 项，失败 {failedCount} 项。", "执行结果", MessageBoxButton.OK, MessageBoxImage.Information);
    }

    private void BackButton_OnClick(object sender, RoutedEventArgs e)
    {
        if (!Confirm("将返回首页并保留当前日志，是否继续？"))
        {
            return;
        }

        OperationPanel.Visibility = Visibility.Collapsed;
        HomePanel.Visibility = Visibility.Visible;
    }

    private void EnterOperationMode()
    {
        HomePanel.Visibility = Visibility.Collapsed;
        OperationPanel.Visibility = Visibility.Visible;
        DestinationPathTextBox.Focus();
    }

    private void UpdateOperationState(SyncOperation operation)
    {
        _currentOperation = operation;

        if (operation == SyncOperation.Create)
        {
            StepTitleText.Text = "步骤：创建软连接";
            ExecuteButton.Content = "开始创建";
        }
        else
        {
            StepTitleText.Text = "步骤：删除软连接";
            ExecuteButton.Content = "开始删除";
        }
    }

    private static bool Confirm(string message)
    {
        return System.Windows.MessageBox.Show(message, "操作确认", MessageBoxButton.YesNo, MessageBoxImage.Question) == MessageBoxResult.Yes;
    }

    private static string? PickFolder()
    {
        using var dialog = new Forms.FolderBrowserDialog
        {
            Description = "请选择 .Obsidian 文件夹",
            ShowNewFolderButton = false,
            UseDescriptionForTitle = true
        };

        return dialog.ShowDialog() == Forms.DialogResult.OK ? dialog.SelectedPath : null;
    }

    private static bool LooksLikeObsidianFolder(string path)
    {
        return path.TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar)
            .EndsWith(".Obsidian", System.StringComparison.OrdinalIgnoreCase);
    }
}

[CmdletBinding()]
param(
    [string]$DockerHubUser,
    [ValidateSet("Separate", "FullStack")]
    [string]$ImageMode = "FullStack"
)

$ErrorActionPreference = "Stop"

$loggedInUser = & podman login --get-login docker.io 2>$null
$podmanLoggedIn = $LASTEXITCODE -eq 0
if ($podmanLoggedIn) {
    $loggedInUser = "$loggedInUser".Trim()
    $podmanLoggedIn = -not [string]::IsNullOrWhiteSpace($loggedInUser)
}

if ([string]::IsNullOrWhiteSpace($DockerHubUser)) {
    if ($podmanLoggedIn) {
        $DockerHubUser = $loggedInUser
    } else {
        $DockerHubUser = Read-Host "Docker Hub 用户名或组织名"
    }
}

$DockerHubUser = $DockerHubUser.Trim()
if ([string]::IsNullOrWhiteSpace($DockerHubUser)) {
    throw "Docker Hub 用户名不能为空。"
}

$repositoryRoot = Split-Path -Parent $PSScriptRoot
$version = Get-Date -Format "yyyyMMdd-HHmmss"

if ($ImageMode -eq "FullStack") {
    $imageTargets = @(
        [pscustomobject]@{ Name = "mty_app"; Dockerfile = (Join-Path $repositoryRoot "Dockerfile") }
    )
} else {
    $imageTargets = @(
        [pscustomobject]@{ Name = "mty_server"; Dockerfile = (Join-Path $repositoryRoot "server/Dockerfile") }
        [pscustomobject]@{ Name = "mty_web"; Dockerfile = (Join-Path $repositoryRoot "web/Dockerfile") }
    )
}

function Invoke-Podman {
    param(
        [string[]]$PodmanArgs
    )

    & podman @PodmanArgs
    if ($LASTEXITCODE -ne 0) {
        throw "podman $($PodmanArgs -join ' ') 执行失败，退出码：$LASTEXITCODE"
    }
}

if ($podmanLoggedIn) {
    Write-Host "已登录 Docker Hub：$loggedInUser"
} else {
    Write-Host "登录 Docker Hub..."
    Invoke-Podman -PodmanArgs @("login", "docker.io", "--username", $DockerHubUser)
}

Write-Host "清理上次构建残留的镜像..."
$existingImageRefs = & podman images --format '{{.Repository}}:{{.Tag}}'
if ($LASTEXITCODE -ne 0) {
    throw "无法列出本地 Podman 镜像，已停止构建。"
}
$ownedImagePattern = "^docker\.io/$([regex]::Escape($DockerHubUser))/(mty_app|mty_server|mty_web):(latest|\d{8}-\d{6})$"
$previousImageRefs = @($existingImageRefs | Where-Object { $_ -match $ownedImagePattern })
if ($previousImageRefs.Count -gt 0) {
    Invoke-Podman -PodmanArgs (@("rmi") + $previousImageRefs)
}

foreach ($image in $imageTargets) {
    $latest = "docker.io/$DockerHubUser/$($image.Name):latest"
    $versioned = "docker.io/$DockerHubUser/$($image.Name):$version"
    Write-Host "构建 $($image.Name) 镜像..."
    Invoke-Podman -PodmanArgs @(
        "build",
        "--file", $image.Dockerfile,
        "--tag", $latest,
        "--tag", $versioned,
        $repositoryRoot
    )
}

foreach ($image in $imageTargets) {
    $latest = "docker.io/$DockerHubUser/$($image.Name):latest"
    $versioned = "docker.io/$DockerHubUser/$($image.Name):$version"
    Write-Host "推送 $($image.Name) 镜像..."
    Invoke-Podman -PodmanArgs @("push", $versioned)
    Invoke-Podman -PodmanArgs @("push", $latest)
}

Write-Host "镜像已推送，删除本地镜像标签..."
$localImageRefs = @(
    foreach ($image in $imageTargets) {
        "docker.io/$DockerHubUser/$($image.Name):$version"
        "docker.io/$DockerHubUser/$($image.Name):latest"
    }
)
Invoke-Podman -PodmanArgs (@("rmi") + $localImageRefs)

Write-Host "完成。已发布模式：$ImageMode；版本：$version"

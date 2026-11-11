// tauri:dev 的启动器：以「免责」方式拉起开发版，让它成为自己的负责进程。
// 否则系统把麦克风等隐私授权算到启动链上的 npm/node 头上，它们没有用途说明，会被静默拒绝。
// 结束信号转发给开发版；启动器被强制结束时，开发版发现父进程变化后自行退出（见 main.rs）。
#include <errno.h>
#include <signal.h>
#include <spawn.h>
#include <stdio.h>
#include <sys/wait.h>

extern char **environ;
// libSystem 导出但未公开的接口，系统自己的 disclaimer 等工具也用它。
extern int responsibility_spawnattrs_setdisclaim(posix_spawnattr_t *attrs, int disclaim);

static pid_t child;

static void forward(int signal_number) {
    if (child > 0) {
        kill(child, signal_number);
    }
}

int main(int argc, char **argv) {
    if (argc < 2) {
        fprintf(stderr, "用法：sayit-dev-launcher <程序> [参数…]\n");
        return 64;
    }
    posix_spawnattr_t attributes;
    posix_spawnattr_init(&attributes);
    responsibility_spawnattrs_setdisclaim(&attributes, 1);
    int error = posix_spawn(&child, argv[1], NULL, &attributes, argv + 1, environ);
    posix_spawnattr_destroy(&attributes);
    if (error != 0) {
        errno = error;
        perror("sayit-dev-launcher");
        return 127;
    }
    signal(SIGTERM, forward);
    signal(SIGINT, forward);
    signal(SIGHUP, forward);
    int status;
    while (waitpid(child, &status, 0) < 0) {
        if (errno != EINTR) {
            return 1;
        }
    }
    return WIFEXITED(status) ? WEXITSTATUS(status) : 128 + WTERMSIG(status);
}

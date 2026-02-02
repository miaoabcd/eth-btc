#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <sys/sendfile.h>
#include <sys/stat.h>
#include <unistd.h>

static int copy_fd(int src_fd, int dst_fd) {
    off_t offset = 0;
    struct stat st;
    if (fstat(src_fd, &st) != 0) {
        return -1;
    }
    off_t remaining = st.st_size;
    while (remaining > 0) {
        ssize_t sent = sendfile(dst_fd, src_fd, &offset, (size_t)remaining);
        if (sent < 0) {
            return -1;
        }
        remaining -= sent;
    }
    return 0;
}

static int fallback_copy_from_path(const char *oldpath, const char *newpath) {
    int src_fd = open(oldpath, O_RDONLY | O_CLOEXEC);
    if (src_fd < 0) {
        return -1;
    }
    struct stat st;
    if (fstat(src_fd, &st) != 0) {
        int saved = errno;
        close(src_fd);
        errno = saved;
        return -1;
    }
    int dst_fd = open(newpath, O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC, st.st_mode & 0777);
    if (dst_fd < 0) {
        int saved = errno;
        close(src_fd);
        errno = saved;
        return -1;
    }
    int result = copy_fd(src_fd, dst_fd);
    int saved = errno;
    close(src_fd);
    close(dst_fd);
    if (result != 0) {
        errno = saved;
        return -1;
    }
    return 0;
}

static int fallback_copy_from_fd(int src_fd_in, int newdirfd, const char *newpath) {
    int src_fd = dup(src_fd_in);
    if (src_fd < 0) {
        return -1;
    }
    struct stat st;
    if (fstat(src_fd, &st) != 0) {
        int saved = errno;
        close(src_fd);
        errno = saved;
        return -1;
    }
    int dst_fd = openat(newdirfd, newpath, O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC, st.st_mode & 0777);
    if (dst_fd < 0) {
        int saved = errno;
        close(src_fd);
        errno = saved;
        return -1;
    }
    int result = copy_fd(src_fd, dst_fd);
    int saved = errno;
    close(src_fd);
    close(dst_fd);
    if (result != 0) {
        errno = saved;
        return -1;
    }
    return 0;
}

static int fallback_move_from_path(const char *oldpath, const char *newpath) {
    int src_fd = open(oldpath, O_RDONLY | O_CLOEXEC);
    if (src_fd < 0) {
        return -1;
    }
    struct stat st;
    if (fstat(src_fd, &st) != 0) {
        int saved = errno;
        close(src_fd);
        errno = saved;
        return -1;
    }
    if (unlink(newpath) != 0 && errno != ENOENT) {
        int saved = errno;
        close(src_fd);
        errno = saved;
        return -1;
    }
    int dst_fd = open(newpath, O_WRONLY | O_CREAT | O_TRUNC | O_CLOEXEC, st.st_mode & 0777);
    if (dst_fd < 0) {
        int saved = errno;
        close(src_fd);
        errno = saved;
        return -1;
    }
    int result = copy_fd(src_fd, dst_fd);
    int saved = errno;
    close(src_fd);
    close(dst_fd);
    if (result != 0) {
        errno = saved;
        return -1;
    }
    if (unlink(oldpath) != 0) {
        return -1;
    }
    return 0;
}

static int fallback_move_at(int olddirfd, const char *oldpath, int newdirfd, const char *newpath) {
    int src_fd = openat(olddirfd, oldpath, O_RDONLY | O_CLOEXEC);
    if (src_fd < 0) {
        return -1;
    }
    struct stat st;
    if (fstat(src_fd, &st) != 0) {
        int saved = errno;
        close(src_fd);
        errno = saved;
        return -1;
    }
    if (unlinkat(newdirfd, newpath, 0) != 0 && errno != ENOENT) {
        int saved = errno;
        close(src_fd);
        errno = saved;
        return -1;
    }
    int dst_fd = openat(newdirfd, newpath, O_WRONLY | O_CREAT | O_TRUNC | O_CLOEXEC, st.st_mode & 0777);
    if (dst_fd < 0) {
        int saved = errno;
        close(src_fd);
        errno = saved;
        return -1;
    }
    int result = copy_fd(src_fd, dst_fd);
    int saved = errno;
    close(src_fd);
    close(dst_fd);
    if (result != 0) {
        errno = saved;
        return -1;
    }
    if (unlinkat(olddirfd, oldpath, 0) != 0) {
        return -1;
    }
    return 0;
}

int link(const char *oldpath, const char *newpath) {
    static int (*real_link)(const char *, const char *) = NULL;
    if (!real_link) {
        real_link = dlsym(RTLD_NEXT, "link");
    }
    int result = real_link(oldpath, newpath);
    if (result == 0 || errno != EXDEV) {
        return result;
    }
    return fallback_copy_from_path(oldpath, newpath);
}

int linkat(int olddirfd, const char *oldpath, int newdirfd, const char *newpath, int flags) {
    static int (*real_linkat)(int, const char *, int, const char *, int) = NULL;
    if (!real_linkat) {
        real_linkat = dlsym(RTLD_NEXT, "linkat");
    }
    int result = real_linkat(olddirfd, oldpath, newdirfd, newpath, flags);
    if (result == 0 || errno != EXDEV) {
        return result;
    }
    if ((flags & AT_EMPTY_PATH) && (!oldpath || oldpath[0] == '\0')) {
        return fallback_copy_from_fd(olddirfd, newdirfd, newpath);
    }
    int src_fd = openat(olddirfd, oldpath, O_RDONLY | O_CLOEXEC);
    if (src_fd < 0) {
        return -1;
    }
    struct stat st;
    if (fstat(src_fd, &st) != 0) {
        int saved = errno;
        close(src_fd);
        errno = saved;
        return -1;
    }
    int dst_fd = openat(newdirfd, newpath, O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC, st.st_mode & 0777);
    if (dst_fd < 0) {
        int saved = errno;
        close(src_fd);
        errno = saved;
        return -1;
    }
    int copy_result = copy_fd(src_fd, dst_fd);
    int saved = errno;
    close(src_fd);
    close(dst_fd);
    if (copy_result != 0) {
        errno = saved;
        return -1;
    }
    return 0;
}

int rename(const char *oldpath, const char *newpath) {
    static int (*real_rename)(const char *, const char *) = NULL;
    if (!real_rename) {
        real_rename = dlsym(RTLD_NEXT, "rename");
    }
    int result = real_rename(oldpath, newpath);
    if (result == 0 || errno != EXDEV) {
        return result;
    }
    return fallback_move_from_path(oldpath, newpath);
}

int renameat(int olddirfd, const char *oldpath, int newdirfd, const char *newpath) {
    static int (*real_renameat)(int, const char *, int, const char *) = NULL;
    if (!real_renameat) {
        real_renameat = dlsym(RTLD_NEXT, "renameat");
    }
    int result = real_renameat(olddirfd, oldpath, newdirfd, newpath);
    if (result == 0 || errno != EXDEV) {
        return result;
    }
    return fallback_move_at(olddirfd, oldpath, newdirfd, newpath);
}

int renameat2(int olddirfd, const char *oldpath, int newdirfd, const char *newpath, unsigned int flags) {
    static int (*real_renameat2)(int, const char *, int, const char *, unsigned int) = NULL;
    if (!real_renameat2) {
        real_renameat2 = dlsym(RTLD_NEXT, "renameat2");
    }
    if (!real_renameat2) {
        if (flags == 0) {
            return renameat(olddirfd, oldpath, newdirfd, newpath);
        }
        errno = ENOSYS;
        return -1;
    }
    int result = real_renameat2(olddirfd, oldpath, newdirfd, newpath, flags);
    if (result == 0 || errno != EXDEV) {
        return result;
    }
    if (flags != 0) {
        return result;
    }
    return fallback_move_at(olddirfd, oldpath, newdirfd, newpath);
}

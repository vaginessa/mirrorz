#ifndef ARGS_H
#define ARGS_H
#pragma once

extern "C"
{
#include <mirror.h>
}

#include <string>
#include <vector>

class Args
{
public:
    struct Params
    {
        std::string encoder = mirror_find_video_encoder();
        std::string decoder = mirror_find_video_decoder();
        std::string server = "127.0.0.1:8080";
        int width = 1280;
        int height = 720;
        int fps = 24;
        int id = 0;
    };

    Args(std::string args);

    struct Params ArgsParams;
private:
    std::vector<std::string> finds(std::string input, std::string delimiter);
};

#endif

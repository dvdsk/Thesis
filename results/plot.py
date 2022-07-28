from dataclasses import dataclass
from collections import defaultdict
from typing import List, Tuple, Dict
import os

import numpy as np
import pandas as pd
from matplotlib import pyplot as plt
import seaborn as sns


@dataclass
class Client:
    numb: int
    start_times: np.ndarray
    end_times: np.ndarray

    def durations(self) -> np.ndarray:
        return self.end_times - self.start_times


def client_from(lines: List[str]) -> Client:
    numb = int(''.join(ch for ch in lines[0] if ch.isdigit()))
    start_times = np.fromstring(lines[1], dtype=float, sep=" ")
    end_times = np.fromstring(lines[2], dtype=float, sep=" ")
    return Client(numb, start_times, end_times)


@dataclass
class Node:
    name: str
    clients: List[Client]

    def durations(self) -> np.ndarray:
        res = [client.durations() for client in self.clients]
        return np.hstack(res)


def node_from(path: str, name: str) -> Node:
    with open(path) as file:
        lines = file.readlines()
    clients = []
    for i in range(0, len(lines), 4):
        clients.append(client_from(lines[i:i+3]))
    return Node(name, clients)


# TODO: switch Node and Run <22-07-22, dvdsk noreply@davidsk.dev>
@dataclass
class Run:
    run: int
    nodes: List[Node]

    def durations(self) -> np.ndarray:
        res = [node.durations() for node in self.nodes]
        return np.hstack(res)


def run_from(dir: str, run: int, nodes: List[str]) -> Run:
    runs = []
    for node in nodes:
        path = f"{dir}/{node}_{run}.csv"
        runs.append(node_from(path, node))
    return Run(run, runs)


@dataclass
class Data:
    runs: List[Run]

    def durations(self) -> np.ndarray:
        res = [node.durations() for node in self.runs]
        return np.hstack(res)

    def flatten(self) -> Tuple[np.ndarray, np.ndarray]:
        start_times = []
        end_times = []
        for run in self.runs:
            for node in run.nodes:
                for client in node.clients:
                    start_times.append(client.start_times)
                    end_times.append(client.end_times)
        return np.hstack(start_times), np.hstack(end_times)


def data_from(dir: str) -> Data:
    nodes_by_run = defaultdict(list)
    for fname in os.listdir(dir):
        node = fname.split("_")[0]
        run = int(fname.split("_")[1].split(".")[0])
        nodes_by_run[run].append(node)

    runs = []
    for run, nodes in nodes_by_run.items():
        runs.append(run_from(dir, run, nodes))
    return Data(runs)


def read_and_writes():
    times = np.loadtxt("data/RangeByRow/1000_1/node117.csv", delimiter=",")
    print(times)

# see: https://stackoverflow.com/questions/46245035/pandas-dataframe-remove-outliers


def remove_outliers(df, col, n_std):
    mean = df[col].mean()
    sd = df[col].std()

    df = df[(df[col] <= mean+(n_std*sd))]

    return df


def Remove_Outlier_Indices(df):
    Q1 = df.quantile(0.25)
    Q3 = df.quantile(0.75)
    IQR = Q3 - Q1
    trueList = ~((df < (Q1 - 1.5 * IQR)) | (df > (Q3 + 1.5 * IQR)))
    return trueList


def catargory_dur(catagories: Dict[str, str], hue_title: str,
                  operation: str, x_label: str, x_values=[1, 2, 3, 4, 5],
                  x_append: str = ""):
    # headers: n_ministeries, duration, access_pattern
    data = {
        x_label: [],
        operation: [],
        hue_title: [],
    }
    print(data)

    for (catarory, dir) in catagories.items():
        for n in x_values:
            durations = data_from(f"data/{dir}/{n}{x_append}").durations()
            data[operation].append(durations)
            data[x_label].append(np.full(durations.size, n))
            data[hue_title].append(np.full(durations.size, catarory))

    for key in data.keys():
        data[key] = np.hstack(data[key])

    data = pd.DataFrame(data)
    filterd = remove_outliers(data, operation, 2)

    plt.plot()

    # plt.yscale("log") # do log when keeping outliers
    # sns.boxplot(x=x_label, y=operation,
    #             hue=hue_title, data=data, showfliers = False)
    # sns.scatterplot(x=x_label, y=operation,
    #                 hue=hue_title, data=data)

    sns.violinplot(x=x_label, y=operation,
                   hue=hue_title, data=filterd)

    # sns.swarmplot(x=x_label, y=operation,
    #               hue=hue_title, data=filterd)

    plt.show()


def dur_dist(bench: str, operation: str):
    # headers: n_ministeries, duration, access_pattern
    data = {
        "number of ministries": [],
        f"{operation} duration": [],
    }
    print(data)

    for n in [1, 2, 3, 4, 5]:
        durations = data_from(f"data/{bench}/{n}").durations()
        data["create duration"].append(durations)
        data["number of ministries"].append(np.full(durations.size, n))

    for key in data.keys():
        data[key] = np.hstack(data[key])

    data = pd.DataFrame(data)
    filterd = remove_outliers(data, "create duration", 4)

    plt.plot()

    # plt.yscale("log") # do log when keeping outliers
    # sns.boxplot(x="number of ministries", y="create duration",
    #             data=data, showfliers=True)

    # sns.violinplot(x="number of ministries", y="create duration", data=data)

    # sns.stripplot(x="number of ministries", y="create duration",
    #               data=data, jitter=0.40)

    sns.histplot(
        data=filterd,
        x="create duration", hue="number of ministries",
        multiple="stack",
        palette=sns.color_palette("Set2", n_colors=5),
        edgecolor=".3",
        linewidth=.5,
        log_scale=False,
    )
    plt.show()


def dur_vs_time(bench: str):
    data = {
        "start_time": [],
        "duration": [],
        "number of ministries": []
    }

    for n in [1, 2, 3, 4, 5]:
        start_times, end_times = data_from(f"data/{bench}/{n}").flatten()
        durations = end_times - start_times
        assert(start_times.size == durations.size)
        data["start_time"].append(start_times)
        data["duration"].append(durations)
        data["number of ministries"].append(np.full(start_times.size, n))

    for key in data.keys():
        data[key] = np.hstack(data[key])

    data = pd.DataFrame(data)

    plt.plot()
    plt.xscale("log")
    sns.scatterplot(data=data, x="start_time", y="duration",
                    hue="number of ministries")
    plt.show()


catargory_dur({"write by row": "RangeByRow", "write entire file": "RangeWholeFile"},
              "write pattern", "write", "row length (Kb)",
              x_values=[1000, 10_000, 100_000, 1_000_000], x_append="_3")
# catargory_dur({"batch": "LsBatch", "stride": "LsStride"},
#               "access pattern", "number of ministries")
# dur_dist("LsStride", "ls")
# dur_dist("Touch", "create")
# dur_vs_time("Touch")

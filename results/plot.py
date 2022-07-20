from dataclasses import dataclass
from typing import List
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
        res = []
        for client in self.clients:
            res.append(client.durations())
        return np.hstack(res)


def node_from(path: str) -> Node:
    name = os.path.basename(path).split(",")[0]
    lines = None
    with open(path) as file:
        lines = file.readlines()
    clients = []
    for i in range(0, len(lines), 4):
        clients.append(client_from(lines[i:i+3]))
    return Node(name, clients)


@dataclass
class Data:
    nodes: List[Node]

    def durations(self) -> np.ndarray:
        res = []
        for node in self.nodes:
            res.append(node.durations())
        return np.hstack(res)


def data_from(dir: str) -> Data:
    nodes = []
    for node in os.listdir(dir):
        f = os.path.join(dir, node)
        nodes.append(node_from(f))
    return Data(nodes)


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


def ls():

    # headers: n_ministeries, duration, access_pattern
    data = {
        "number of ministries": [],
        "ls duration": [],
        "access pattern": [],
    }
    print(data)

    for (pattern, dir) in [("batch", "LsBatch"), ("stride", "LsStride")]:
        for n in [1, 2, 3, 4, 5]:
            durations = data_from(f"data/{dir}/{n}").durations()
            data["ls duration"].append(durations)
            data["number of ministries"].append(np.full(durations.size, n))
            data["access pattern"].append(np.full(durations.size, pattern))

    for key in data.keys():
        data[key] = np.hstack(data[key])

    data = pd.DataFrame(data)
    filterd = remove_outliers(data, "ls duration", 2)

    plt.plot()

    # plt.yscale("log") # do log when keeping outliers
    # sns.boxplot(x="number of ministries", y="ls duration",
    #             hue="access pattern", data=data, showfliers = False)

    sns.violinplot(x="number of ministries", y="ls duration",
                   hue="access pattern", data=filterd)

    # sns.swarmplot(x="number of ministries", y="ls duration",
    #               hue="access pattern", data=filterd)

    plt.show()


ls()
